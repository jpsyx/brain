import csv
import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
import uuid
from pathlib import Path


SCRIPTS = Path(__file__).resolve().parents[1]
ADD_TASK = SCRIPTS / "add_task.py"
REASSIGN_TASK = SCRIPTS / "reassign_task.py"
NEXT_OCCURRENCE = SCRIPTS / "next_habit_occurrence.py"
APPLY_SYNC_RULES = SCRIPTS / "apply_sync_rules.py"
CLEANUP_DONE_HABITS = SCRIPTS / "cleanup_done_habits.py"
NEXT_ID = SCRIPTS / "next_id.py"
BAKE_APPENDIX = SCRIPTS / "bake_triage_appendix.py"
WORKSPACE_ID = "e806258e-491a-436d-9db4-a5ca9903e0d4"


class WorkspaceContextTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)
        self.home = self.base / "home"
        self.root = self.base / "family"
        self.xdg = self.base / "xdg"
        (self.home / "brain" / "tasks").mkdir(parents=True)
        (self.root / "tasks").mkdir(parents=True)
        (self.root / ".config").mkdir(parents=True)
        (self.xdg / "brain").mkdir(parents=True)
        (self.home / "brain" / "tasks" / "tasks.csv").write_text(
            "task_id,task_name\nSENTINEL,Do not touch\n", encoding="utf-8"
        )
        (self.root / ".config" / "users.json").write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "users": [
                        {"id": "pablo", "name": "Pablo"},
                        {"id": "wife", "name": "Wife"},
                    ],
                }
            )
            + "\n",
            encoding="utf-8",
        )
        (self.xdg / "brain" / "env.json").write_text("{}\n", encoding="utf-8")

    def tearDown(self):
        self.temporary.cleanup()

    def env(self, *, include_root=True):
        environment = {
            "HOME": str(self.home),
            "XDG_CONFIG_HOME": str(self.xdg),
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "TMPDIR": str(self.base / "tmp"),
            "PYTHONDONTWRITEBYTECODE": "1",
            "BRAIN_WORKSPACE": "family",
            "BRAIN_WORKSPACE_ID": WORKSPACE_ID,
            "BRAIN_ACTOR_ID": "wife",
        }
        if include_root:
            environment["BRAIN_ROOT"] = str(self.root)
        return environment

    def run_add(self, *extra, include_root=True):
        return subprocess.run(
            [
                sys.executable,
                str(ADD_TASK),
                "--name",
                "Buy groceries",
                "--type",
                "personal",
                "--priority",
                "p2",
                *extra,
            ],
            env=self.env(include_root=include_root),
            text=True,
            capture_output=True,
            check=False,
        )

    def test_add_task_writes_only_selected_workspace_and_defaults_to_actor(self):
        before = (self.home / "brain" / "tasks" / "tasks.csv").read_bytes()

        result = self.run_add()

        self.assertEqual(result.returncode, 0, result.stderr)
        with (self.root / "tasks" / "tasks.csv").open(newline="") as handle:
            rows = list(csv.DictReader(handle))
        self.assertEqual(rows[0]["assigned_to"], "wife")
        self.assertEqual(uuid.UUID(rows[0]["task_uuid"]).version, 4)
        self.assertEqual(
            (self.home / "brain" / "tasks" / "tasks.csv").read_bytes(), before
        )

    def test_missing_brain_root_fails_without_home_fallback(self):
        before = (self.home / "brain" / "tasks" / "tasks.csv").read_bytes()

        result = self.run_add(include_root=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("launch this script through Brain", result.stderr)
        self.assertEqual(
            (self.home / "brain" / "tasks" / "tasks.csv").read_bytes(), before
        )

    def test_csv_writer_rejects_a_stale_snapshot_instead_of_losing_a_concurrent_change(self):
        tasks = self.root / "tasks" / "tasks.csv"
        tasks.write_text(
            "task_id,task_name,status,system_key\nT1,Original,not_started,\n",
            encoding="utf-8",
        )
        program = (
            "from _csvlib import read_csv, write_csv, tasks_csv\n"
            "path = tasks_csv()\n"
            "columns, rows = read_csv(path)\n"
            "path.write_text('task_id,task_name,status,system_key\\nT1,Concurrent,not_started,\\n', encoding='utf-8')\n"
            "rows[0]['task_name'] = 'Stale writer'\n"
            "write_csv(path, columns, rows)\n"
        )

        result = subprocess.run(
            [sys.executable, "-c", program],
            cwd=SCRIPTS,
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Concurrent", tasks.read_text(encoding="utf-8"))

    def test_csv_writer_rejects_deleting_an_enabled_managed_triage_row(self):
        tasks = self.root / "tasks" / "habits.csv"
        tasks.write_text(
            "task_id,task_name,status,system_key\n"
            "H1,Morning Triage,not_started,brain.triage.daily\n",
            encoding="utf-8",
        )
        (self.root / ".config" / "config.json").write_text(
            '{"enable_triage_habits":true}\n', encoding="utf-8"
        )
        program = (
            "from _csvlib import read_csv, write_csv, habits_csv\n"
            "path = habits_csv()\n"
            "columns, rows = read_csv(path)\n"
            "write_csv(path, columns, [])\n"
        )

        result = subprocess.run(
            [sys.executable, "-c", program],
            cwd=SCRIPTS,
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("brain.triage.daily", tasks.read_text(encoding="utf-8"))

    def test_csv_writer_rejects_replacing_a_legacy_managed_row_with_the_same_display_id(self):
        habits = self.root / "tasks" / "habits.csv"
        habits.write_text(
            "task_id,task_name,status,system_key\n"
            "H1,Morning Triage,not_started,brain.triage.daily\n",
            encoding="utf-8",
        )
        (self.root / ".config" / "config.json").write_text(
            '{"enable_triage_habits":true}\n', encoding="utf-8"
        )
        program = (
            "from _csvlib import read_csv, write_csv, habits_csv\n"
            "path = habits_csv()\n"
            "columns, rows = read_csv(path)\n"
            "rows[0]['task_name'] = 'Ordinary replacement'\n"
            "rows[0]['system_key'] = ''\n"
            "write_csv(path, columns, rows)\n"
        )

        result = subprocess.run(
            [sys.executable, "-c", program],
            cwd=SCRIPTS,
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("brain.triage.daily", habits.read_text(encoding="utf-8"))

    def test_concurrent_id_allocators_share_the_workspace_task_lock(self):
        tasks = self.root / "tasks" / "tasks.csv"
        tasks.write_text("task_id,task_name\n", encoding="utf-8")
        processes = [
            subprocess.Popen(
                [sys.executable, str(NEXT_ID), "--kind", "tasks"],
                cwd=SCRIPTS,
                env=self.env(),
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            for _ in range(12)
        ]

        outputs = []
        for process in processes:
            stdout, stderr = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 0, stderr)
            outputs.append(stdout.strip())

        self.assertEqual(len(set(outputs)), 12)
        self.assertEqual(set(outputs), {f"T{number}" for number in range(1, 13)})
        self.assertEqual(
            (self.root / "tasks" / ".tasks_next_id").read_text().strip(), "13"
        )

    def test_explicit_assignment_must_name_a_portable_member(self):
        accepted = self.run_add("--assigned-to", "pablo")
        rejected = self.run_add("--assigned-to", "stranger")

        self.assertEqual(accepted.returncode, 0, accepted.stderr)
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("not a workspace member", rejected.stderr)

    def test_unrelated_writer_migrates_legacy_assignment_header(self):
        tasks = self.root / "tasks" / "tasks.csv"
        tasks.write_text(
            "task_id,task_name,task_type,status,priority,assignee\n"
            "T1,Existing,personal,not_started,p2,pablo\n",
            encoding="utf-8",
        )

        result = self.run_add()

        self.assertEqual(result.returncode, 0, result.stderr)
        with tasks.open(newline="") as handle:
            reader = csv.DictReader(handle)
            rows = list(reader)
            self.assertIn("assigned_to", reader.fieldnames)
            self.assertNotIn("assignee", reader.fieldnames)
        self.assertEqual(rows[0]["assigned_to"], "pablo")
        self.assertEqual(rows[1]["assigned_to"], "wife")

    def test_add_task_appends_canonical_assignment_when_legacy_csv_has_no_assignment(self):
        tasks = self.root / "tasks" / "tasks.csv"
        tasks.write_text(
            "task_id,task_name,task_type,status,priority\n"
            "T1,Existing,personal,not_started,p2\n",
            encoding="utf-8",
        )

        result = self.run_add()

        self.assertEqual(result.returncode, 0, result.stderr)
        with tasks.open(newline="") as handle:
            reader = csv.DictReader(handle)
            rows = list(reader)
            self.assertIn("assigned_to", reader.fieldnames)
        self.assertEqual(rows[0]["assigned_to"], "")
        self.assertEqual(rows[1]["assigned_to"], "wife")

    def test_add_to_legacy_csv_keeps_display_id_as_the_sync_key_until_rollout(self):
        tasks = self.root / "tasks" / "tasks.csv"
        tasks.write_text(
            "task_id,task_name,task_type,status,priority,assigned_to\n"
            "T1,Existing,personal,not_started,p2,pablo\n",
            encoding="utf-8",
        )

        result = self.run_add()

        self.assertEqual(result.returncode, 0, result.stderr)
        with tasks.open(newline="") as handle:
            reader = csv.DictReader(handle)
            rows = list(reader)
        self.assertEqual(reader.fieldnames[0], "task_id")
        self.assertIn("task_uuid", reader.fieldnames)
        self.assertEqual(rows[0]["task_uuid"], "")
        self.assertEqual(uuid.UUID(rows[1]["task_uuid"]).version, 4)

    def test_add_task_uses_the_full_schema_for_an_empty_csv(self):
        tasks = self.root / "tasks" / "tasks.csv"
        tasks.write_text("", encoding="utf-8")

        result = self.run_add()

        self.assertEqual(result.returncode, 0, result.stderr)
        with tasks.open(newline="") as handle:
            reader = csv.DictReader(handle)
            rows = list(reader)
        self.assertIn("task_id", reader.fieldnames)
        self.assertIn("assigned_to", reader.fieldnames)
        self.assertEqual(rows[0]["assigned_to"], "wife")

    def test_reassignment_validates_membership_and_preserves_other_fields(self):
        tasks = self.root / "tasks" / "tasks.csv"
        tasks.write_text(
            "task_uuid,task_id,task_name,status,assigned_to,notes\n"
            "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,T1,Existing,not_started,pablo,keep this\n",
            encoding="utf-8",
        )

        accepted = subprocess.run(
            [sys.executable, str(REASSIGN_TASK), "T1", "wife"],
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )
        rejected = subprocess.run(
            [sys.executable, str(REASSIGN_TASK), "T1", "stranger"],
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(accepted.returncode, 0, accepted.stderr)
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("not a workspace member", rejected.stderr)
        with tasks.open(newline="") as handle:
            row = next(csv.DictReader(handle))
        self.assertEqual(row["assigned_to"], "wife")
        self.assertEqual(row["notes"], "keep this")
        self.assertEqual(
            row["task_uuid"], "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4"
        )

    def test_habit_occurrence_gets_a_new_uuid_and_retains_system_key_and_assignment(self):
        source_uuid = "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4"
        row = {
            "task_uuid": source_uuid,
            "task_id": "H1",
            "task_name": "Morning triage",
            "status": "done",
            "assigned_to": "wife",
            "system_key": "brain.triage.daily",
            "due_date": "2026-08-03",
            "recur_interval": "1",
            "recur_unit": "days",
        }
        result = subprocess.run(
            [sys.executable, str(NEXT_OCCURRENCE)],
            cwd=SCRIPTS,
            env=self.env(),
            input=json.dumps(row),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        occurrence = json.loads(result.stdout)
        self.assertEqual(uuid.UUID(occurrence["task_uuid"]).version, 4)
        self.assertNotEqual(occurrence["task_uuid"], source_uuid)
        self.assertEqual(occurrence["system_key"], "brain.triage.daily")
        self.assertEqual(occurrence["assigned_to"], "wife")

    def test_csvlib_exposes_uuid_creation_without_adding_uuid_columns(self):
        result = subprocess.run(
            [
                sys.executable,
                "-c",
                "from _csvlib import new_uuid; print(new_uuid())",
            ],
            cwd=SCRIPTS,
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        uuid.UUID(result.stdout.strip())

    def test_project_metadata_writer_rejects_a_stale_snapshot(self):
        metadata = self.root / "projects" / "alpha" / ".METADATA.json"
        metadata.parent.mkdir(parents=True)
        metadata.write_text('{"name":"alpha","tasks":[]}\n', encoding="utf-8")
        program = (
            "import os\n"
            "from pathlib import Path\n"
            "from apply_sync_rules import load_json, save_json\n"
            "path = Path(os.environ['METADATA_PATH'])\n"
            "value = load_json(path)\n"
            "path.write_text('{\"name\":\"alpha\",\"tasks\":[],\"owner\":\"concurrent\"}\\n', encoding='utf-8')\n"
            "value['tasks'] = ['T1']\n"
            "save_json(path, value)\n"
        )
        environment = self.env()
        environment["METADATA_PATH"] = str(metadata)

        result = subprocess.run(
            [sys.executable, "-c", program],
            cwd=SCRIPTS,
            env=environment,
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(
            json.loads(metadata.read_text(encoding="utf-8"))["owner"],
            "concurrent",
        )

    def test_project_metadata_writer_waits_for_the_shared_task_store_owner(self):
        metadata = self.root / "projects" / "alpha" / ".METADATA.json"
        metadata.parent.mkdir(parents=True)
        metadata.write_text('{"name":"alpha","tasks":[]}\n', encoding="utf-8")
        ready = self.base / "metadata-ready"
        lock_path = (
            self.home
            / ".cache"
            / "brain"
            / "workspaces"
            / WORKSPACE_ID
            / "tasks.transaction.lock"
        )
        lock_path.parent.mkdir(parents=True)
        owner = sqlite3.connect(lock_path, timeout=30, isolation_level=None)
        owner.execute("PRAGMA journal_mode = OFF")
        owner.execute("BEGIN IMMEDIATE")
        program = (
            "import os\n"
            "from pathlib import Path\n"
            "from apply_sync_rules import load_json, save_json\n"
            "path = Path(os.environ['METADATA_PATH'])\n"
            "value = load_json(path)\n"
            "Path(os.environ['READY_PATH']).write_text('ready', encoding='utf-8')\n"
            "value['tasks'] = ['T1']\n"
            "save_json(path, value)\n"
        )
        environment = self.env()
        environment["METADATA_PATH"] = str(metadata)
        environment["READY_PATH"] = str(ready)
        process = subprocess.Popen(
            [sys.executable, "-c", program],
            cwd=SCRIPTS,
            env=environment,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        deadline = time.monotonic() + 2
        while not ready.exists() and process.poll() is None and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertTrue(ready.exists(), "metadata writer never reached its save boundary")

        with self.assertRaises(subprocess.TimeoutExpired):
            process.wait(timeout=0.2)
        owner.rollback()
        owner.close()
        stdout, stderr = process.communicate(timeout=2)

        self.assertEqual(process.returncode, 0, f"stdout: {stdout}\nstderr: {stderr}")
        self.assertEqual(json.loads(metadata.read_text(encoding="utf-8"))["tasks"], ["T1"])

    def write_managed_habits(self):
        path = self.root / "tasks" / "habits.csv"
        path.write_text(
            "task_uuid,task_id,task_name,status,due_date,assigned_to,"
            "recur_interval,recur_unit,created_date,completed_date,last_touched,system_key\n"
            "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,H1,Morning Triage,not_started,"
            "2026-08-03,wife,1,days,2026-08-03,,,brain.triage.daily\n"
            "7d49b547-1d9f-439b-bd97-b98327ecae20,H2,Morning Triage,not_started,"
            "2026-08-03,wife,1,days,2026-08-03,,,\n",
            encoding="utf-8",
        )
        return path

    def test_managed_triage_completion_preserves_marker_assignment_and_fresh_uuid(self):
        path = self.write_managed_habits()
        (self.root / ".config" / "config.json").write_text(
            '{"enable_triage_habits": true}\n', encoding="utf-8"
        )

        result = subprocess.run(
            [
                sys.executable,
                str(APPLY_SYNC_RULES),
                "--complete-managed-triage",
                "daily",
            ],
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        with path.open(newline="") as handle:
            rows = list(csv.DictReader(handle))
        managed = [
            row for row in rows if row["system_key"] == "brain.triage.daily"
        ]
        self.assertEqual(len(managed), 2)
        self.assertEqual(managed[0]["status"], "done")
        self.assertEqual(managed[1]["status"], "not_started")
        self.assertNotEqual(managed[1]["task_uuid"], managed[0]["task_uuid"])
        self.assertEqual(uuid.UUID(managed[1]["task_uuid"]).version, 4)
        self.assertEqual(managed[1]["assigned_to"], "wife")
        self.assertEqual(
            rows[1]["task_uuid"], "7d49b547-1d9f-439b-bd97-b98327ecae20"
        )

    def test_managed_triage_completion_is_a_noop_when_feature_is_disabled(self):
        path = self.write_managed_habits()
        before = path.read_bytes()
        (self.root / ".config" / "config.json").write_text(
            '{"enable_triage_habits": false}\n', encoding="utf-8"
        )

        result = subprocess.run(
            [
                sys.executable,
                str(APPLY_SYNC_RULES),
                "--complete-managed-triage",
                "daily",
            ],
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(path.read_bytes(), before)
        self.assertIn("disabled", result.stdout)

    def test_cleanup_preserves_managed_history_for_transactional_policy(self):
        path = self.root / "tasks" / "habits.csv"
        path.write_text(
            "task_uuid,task_id,task_name,status,due_date,assigned_to,"
            "recur_interval,recur_unit,created_date,completed_date,last_touched,system_key\n"
            "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,H1,Morning Triage,done,"
            "2000-01-01,wife,1,days,2000-01-01,2000-01-01,2000-01-01,brain.triage.daily\n"
            "7d49b547-1d9f-439b-bd97-b98327ecae20,H2,Morning Triage,not_started,"
            "2000-01-02,wife,1,days,2000-01-01,,2000-01-01,brain.triage.daily\n",
            encoding="utf-8",
        )
        (self.root / ".config" / "config.json").write_text(
            '{"enable_triage_habits": true}\n', encoding="utf-8"
        )

        result = subprocess.run(
            [sys.executable, str(CLEANUP_DONE_HABITS)],
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        with path.open(newline="") as handle:
            rows = list(csv.DictReader(handle))
        self.assertEqual([row["task_id"] for row in rows], ["H1", "H2"])

    def test_cleanup_does_not_attempt_partial_managed_purge_when_disabled(self):
        path = self.root / "tasks" / "habits.csv"
        path.write_text(
            "task_uuid,task_id,task_name,status,due_date,assigned_to,"
            "recur_interval,recur_unit,created_date,completed_date,last_touched,system_key\n"
            "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,H1,Morning Triage,done,"
            "2000-01-01,wife,1,days,2000-01-01,2000-01-01,2000-01-01,brain.triage.daily\n",
            encoding="utf-8",
        )
        before = path.read_bytes()
        (self.root / ".config" / "config.json").write_text(
            '{"enable_triage_habits": false}\n', encoding="utf-8"
        )

        result = subprocess.run(
            [sys.executable, str(CLEANUP_DONE_HABITS)],
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(path.read_bytes(), before)
        self.assertIn("transactional", result.stdout)

    def test_bundled_scripts_do_not_embed_home_brain_or_extension_sources(self):
        offenders = []
        forbidden = (
            '~/brain',
            'Path.home() / "brain"',
            "Path.home() / 'brain'",
            'email-triage',
            'newsletter',
            'agenda-appendix',
            '## 📧',
            '## 📰',
        )
        targets = [SCRIPTS.parent / "SKILL.md", *SCRIPTS.glob("*.py")]
        for target in targets:
            source = target.read_text(encoding="utf-8").lower()
            if any(value.lower() in source for value in forbidden):
                offenders.append(target.name)
        self.assertEqual(offenders, [])

    def test_appendix_baker_uses_only_caller_supplied_content_and_paths(self):
        agenda = self.base / "agenda.md"
        content = self.base / "optional.md"
        agenda.write_text("# Agenda\n\n## Suggested order\n\n1. Work\n", encoding="utf-8")
        content.write_text("# Optional title\n\nCaller content\n", encoding="utf-8")

        first = subprocess.run(
            [
                sys.executable,
                str(BAKE_APPENDIX),
                "--agenda",
                str(agenda),
                "--content",
                str(content),
            ],
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertIn("## Appendix <!-- brain:optional-content -->", agenda.read_text())
        self.assertIn("Caller content", agenda.read_text())
        content.write_text("Replacement content\n", encoding="utf-8")
        second = subprocess.run(
            [
                sys.executable,
                str(BAKE_APPENDIX),
                "--agenda",
                str(agenda),
                "--content",
                str(content),
            ],
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )
        rendered = agenda.read_text(encoding="utf-8")
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(rendered.count("<!-- brain:optional-content -->"), 1)
        self.assertNotIn("Caller content", rendered)
        self.assertIn("Replacement content", rendered)

    def test_agenda_mutation_inserts_core_sections_before_generic_optional_content(self):
        program = (
            "from update_agenda_on_mutation import "
            "H_APPENDIX_PREFIX, _replace_or_set_section, _split_sections\n"
            "_, sections = _split_sections("
            "'## Suggested order\\n\\n1. Work\\n\\n' + H_APPENDIX_PREFIX + '\\n\\nOptional\\n')\n"
            "_replace_or_set_section(sections, '## ✅', ['## ✅ Completed today', ['', 'Done']])\n"
            "print('\\n'.join(section[0] for section in sections))\n"
        )

        result = subprocess.run(
            [sys.executable, "-c", program],
            cwd=SCRIPTS,
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stdout.splitlines(),
            ["## Suggested order", "## ✅ Completed today", "## Appendix <!-- brain:optional-content -->"],
        )


if __name__ == "__main__":
    unittest.main()
