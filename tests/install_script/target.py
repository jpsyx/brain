#!/usr/bin/env python3
import hashlib, json, os, pathlib, subprocess, sys
version, identity = '@VERSION@', '@IDENTITY@'
args = sys.argv[1:]
if args == ['--version']:
    print('brain ' + version); sys.exit()
entry = {'identity': identity, 'args': args, 'executable': sys.argv[0]}
if args and args[0] == '__refresh-workspace-skills' and version != '0.101.0':
    values = dict(zip(args[1::2], args[2::2]))
    entry.update(phase=values['--phase'], target=values['--target-binary'], digest=values['--expected-target-sha256'])
    assert hashlib.sha256(pathlib.Path(entry['target']).read_bytes()).hexdigest() == entry['digest']
elif args and args[0] == '__migrate' and version != '0.101.0':
    pass
elif args != ['-w', 'canonical', 'skills', 'sync']:
    sys.exit('unsupported target argv')
with open(os.environ['MIGRATION_LOG'], 'a') as log:
    log.write(json.dumps(entry) + '\n')
if entry.get('phase') == os.environ.get('FAIL_PHASE', '-') or (args[0] == '__migrate' and os.environ.get('FAIL_PHASE') == 'down'):
    sys.exit('canonical: pending failure')
if entry.get('phase') == 'preflight' and os.environ.get('MUTATE_BUILD'):
    pathlib.Path(os.environ['MUTATE_BUILD']).write_text('changed mutable cargo artifact')
if entry.get('phase') == 'apply':
    if os.environ.get('FAIL_PHASE') == 'down':
        sys.exit('migration failure')
    subprocess.run([entry['target'], '-w', 'canonical', 'skills', 'sync'], check=True)
    if '--replace-binary' in args:
        if os.environ.get('FAIL_RENAME'):
            sys.exit('replacement failure')
        os.replace(entry['target'], values['--replace-binary'])
if entry.get('phase') == 'preflight' and os.environ.get('MUTATE_CANDIDATE'):
    with open(entry['target'], 'a') as candidate:
        candidate.write('\n# replaced same-version candidate\n')
