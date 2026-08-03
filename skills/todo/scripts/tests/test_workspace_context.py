import csv
import json
import os
import subprocess
import sys
import tempfile
import unittest
import uuid
from pathlib import Path


SCRIPTS = Path(__file__).resolve().parents[1]
ADD_TASK = SCRIPTS / "add_task.py"
REASSIGN_TASK = SCRIPTS / "reassign_task.py"
NEXT_OCCURRENCE = SCRIPTS / "next_habit_occurrence.py"
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

    def test_bundled_scripts_do_not_embed_a_home_brain_fallback(self):
        offenders = []
        for script in SCRIPTS.glob("*.py"):
            source = script.read_text(encoding="utf-8")
            if 'Path.home() / "brain"' in source or "Path.home() / 'brain'" in source:
                offenders.append(script.name)
        self.assertEqual(offenders, [])


if __name__ == "__main__":
    unittest.main()
