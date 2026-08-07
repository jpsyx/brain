"""Habits are never collateral damage of a task cleanup.

`remove_task.py` resolves a needle through `_csvlib.locate`, which searches
`tasks.csv` *and* `habits.csv`. Triage's past-due passes only ever mean to drop
tasks, so a needle that lands on a habit row must be refused rather than
silently deleting a live recurring chain (which takes every future occurrence
with it). Removing a habit stays possible, but only when the caller says so
explicitly with `--habit`.
"""

import csv
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPTS = Path(__file__).resolve().parents[1]
REMOVE_TASK = SCRIPTS / "remove_task.py"
WORKSPACE_ID = "e806258e-491a-436d-9db4-a5ca9903e0d4"

HABIT_HEADER = (
    "task_uuid,task_id,task_name,status,due_date,assigned_to,"
    "recur_interval,recur_unit,completed_date,system_key\n"
)
HABIT_ROW = (
    "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,H7,Workout,not_started,"
    "2026-07-15,wife,1,days,,\n"
)


class HabitProtectionTests(unittest.TestCase):
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
        (self.root / ".config" / "users.json").write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "users": [{"id": "pablo", "name": "Pablo"}, {"id": "wife", "name": "Wife"}],
                }
            )
            + "\n",
            encoding="utf-8",
        )
        (self.xdg / "brain" / "env.json").write_text("{}\n", encoding="utf-8")
        self.tasks = self.root / "tasks" / "tasks.csv"
        self.habits = self.root / "tasks" / "habits.csv"
        self.tasks.write_text(
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n"
            "3f1c9d0e-2b7a-4c55-9f2e-6d1a8b4c7e90,T4,File the receipts,not_started,wife,\n",
            encoding="utf-8",
        )
        self.habits.write_text(HABIT_HEADER + HABIT_ROW, encoding="utf-8")

    def tearDown(self):
        self.temporary.cleanup()

    def env(self):
        return {
            "HOME": str(self.home),
            "XDG_CONFIG_HOME": str(self.xdg),
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "TMPDIR": str(self.base / "tmp"),
            "PYTHONDONTWRITEBYTECODE": "1",
            "BRAIN_ROOT": str(self.root),
            "BRAIN_WORKSPACE": "family",
            "BRAIN_WORKSPACE_ID": WORKSPACE_ID,
            "BRAIN_ACTOR_ID": "wife",
        }

    def run_remove(self, *args):
        return subprocess.run(
            [sys.executable, str(REMOVE_TASK), *args],
            env=self.env(),
            text=True,
            capture_output=True,
            check=False,
        )

    def habit_rows(self):
        with self.habits.open(newline="") as handle:
            return list(csv.DictReader(handle))

    def test_removing_a_habit_by_id_is_refused_and_leaves_the_chain_intact(self):
        before = self.habits.read_bytes()

        result = self.run_remove("H7")

        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("habit", result.stderr.lower())
        self.assertEqual(self.habits.read_bytes(), before)

    def test_removing_a_habit_by_fuzzy_name_is_refused(self):
        before = self.habits.read_bytes()

        result = self.run_remove("Workout")

        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(self.habits.read_bytes(), before)

    def test_the_refusal_names_the_habit_and_points_at_the_explicit_flag(self):
        result = self.run_remove("H7")

        self.assertIn("H7", result.stderr)
        self.assertIn("--habit", result.stderr)

    def test_an_explicit_habit_removal_still_works(self):
        result = self.run_remove("H7", "--habit")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.habit_rows(), [])

    def test_removing_an_ordinary_task_is_unaffected(self):
        habits_before = self.habits.read_bytes()

        result = self.run_remove("T4")

        self.assertEqual(result.returncode, 0, result.stderr)
        with self.tasks.open(newline="") as handle:
            self.assertEqual(list(csv.DictReader(handle)), [])
        self.assertEqual(self.habits.read_bytes(), habits_before)

    def test_the_habit_flag_does_not_let_a_task_needle_through_by_accident(self):
        """`--habit` narrows the intent to habits; it must not remove a task."""
        before = self.tasks.read_bytes()

        result = self.run_remove("T4", "--habit")

        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(self.tasks.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
