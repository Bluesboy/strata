# SPDX-License-Identifier: MIT

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from e2e_mutation_result import detected


class MutationRunnerTests(unittest.TestCase):
    def test_parallel_mutation_runs_finish_without_fail_fast_interruption(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = root / "scripts"
            scripts.mkdir()
            for name in ("e2e-mutation-check.sh", "e2e_mutation_result.py"):
                shutil.copyfile(Path(__file__).with_name(name), scripts / name)
            mutations = root / "tests/e2e/mutations"
            mutations.mkdir(parents=True)
            (mutations / "click-modes.patch").write_text(PATCH)
            tools = root / "bin"
            tools.mkdir()
            git = tools / "git"
            git.write_text("#!/bin/sh\nexit 0\n")
            git.chmod(0o755)
            runner = scripts / "e2e.sh"
            runner.write_text(
                f"#!{sys.executable}\nimport sys\nfrom pathlib import Path\n"
                "for arg in sys.argv[1:]:\n"
                "    if arg.startswith('--junitxml='):\n"
                "        Path(arg.split('=', 1)[1]).write_text('<testsuite><testcase><failure/></testcase></testsuite>')\n"
                "        sys.exit(1 if '--maxfail=0' in sys.argv and '-x' not in sys.argv else 2)\n"
            )
            runner.chmod(0o755)
            environment = {**os.environ, "PATH": f"{tools}:{os.environ.get('PATH', os.defpath)}"}
            environment.pop("STRATA_BINARY", None)
            result = subprocess.run(["bash", str(scripts / "e2e-mutation-check.sh"), "click-modes"],
                                    env=environment, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("every mutation was detected", result.stdout)


    def test_qa_mode_requires_both_sides_to_fail_run_by_run(self):
        twin = "tests/e2e/scenarios/test_clipboard.py::test_copy_leaves_the_source_in_place"
        # Columns fails on both sides under the patch. Icons fails in QA, and in
        # Python only when the twin also fails; a truncated QA log never reports it.
        cases = [
            ("python fails in icons", True, True, 0),
            ("only QA fails in icons", False, True, 1),
            ("the QA icons run never reports", True, False, 1),
        ]
        for label, python_fails_in_icons, qa_reports_icons, expected in cases:
            with self.subTest(label), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                scripts = root / "scripts"
                scripts.mkdir()
                for name in ("e2e-mutation-check.sh", "e2e_mutation_result.py", "qa_parity.py"):
                    shutil.copyfile(Path(__file__).with_name(name), scripts / name)
                (root / "tests/e2e/mutations").mkdir(parents=True)
                (root / "tests/e2e/mutations/clipboard.patch").write_text(PATCH)
                (root / "tests/qa/scenarios").mkdir(parents=True)
                (root / "tests/qa/scenarios/copy-leaves-source.jsonl").write_text(
                    '{"scenario":"copy-leaves-source","mutation":"clipboard"}\n'
                )
                applied = root / "applied"
                tools = root / "bin"
                tools.mkdir()
                fake(tools / "git", f"#!/bin/sh\ncase \"$*\" in\n"
                     f"  *apply*) touch {applied} ;;\n  *restore*) rm -f {applied} ;;\nesac\nexit 0\n")
                icons = (f"  echo 'copy-leaves-source/icons (twin {twin}[icons]) failed:'\n"
                         if qa_reports_icons else "")
                fake(scripts / "qa.sh", f"#!/bin/sh\n"
                     f"echo 'RUN copy-leaves-source/columns (twin {twin}[columns])'\n"
                     f"echo 'RUN copy-leaves-source/icons (twin {twin}[icons])'\n"
                     f"if [ -e {applied} ]; then\n"
                     f"  echo 'copy-leaves-source/columns (twin {twin}[columns]) failed:'\n"
                     f"{icons}  exit 101\n"
                     f"fi\n"
                     f"echo 'PASS copy-leaves-source/columns (twin {twin}[columns])'\n"
                     f"echo 'PASS copy-leaves-source/icons (twin {twin}[icons])'\n")
                failing = ["columns", "icons"] if python_fails_in_icons else ["columns"]
                fake(scripts / "e2e.sh", f"#!{sys.executable}\nimport sys\nfrom pathlib import Path\n"
                     f"mutated = Path({str(applied)!r}).exists()\n"
                     "cases = ''\n"
                     "for mode in ('columns', 'icons'):\n"
                     f"    failed = mutated and mode in {failing!r}\n"
                     "    cases += f'<testcase classname=\"tests.e2e.scenarios.test_clipboard\" name=\"test_copy_leaves_the_source_in_place[{mode}]\">'\n"
                     "    cases += ('<failure message=\"x\"/>' if failed else '') + '</testcase>'\n"
                     "for arg in sys.argv[1:]:\n"
                     "    if arg.startswith('--junitxml='):\n"
                     "        Path(arg.split('=', 1)[1]).write_text(f'<testsuite>{cases}</testsuite>')\n"
                     "sys.exit(1 if mutated else 0)\n")
                environment = {**os.environ, "PATH": f"{tools}:{os.environ.get('PATH', os.defpath)}"}
                environment.pop("STRATA_BINARY", None)
                result = subprocess.run(
                    ["bash", str(scripts / "e2e-mutation-check.sh"), "--qa", "clipboard"],
                    env=environment, capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                self.assertFalse(applied.exists(), "the patched files are restored")


PATCH = "--- a/src/ui/browser.rs\n+++ b/src/ui/browser.rs\n"


def fake(path, text):
    path.write_text(text)
    path.chmod(0o755)


class MutationResultTests(unittest.TestCase):
    def test_only_scenario_failures_count_as_detection(self):
        cases = [
            (1, '<failure message="assert False"/>', True),
            (1, '<error message="startup failed"/>', False),
            (1, '<failure/><error/>', False),
            (0, '', False),
            (2, '<failure/>', False),
            (5, '', False),
            (137, '<failure/>', False),
        ]
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.xml"
            for code, outcome, expected in cases:
                with self.subTest(code=code, outcome=outcome):
                    report.write_text(
                        f'<testsuites><testsuite><testcase>{outcome}'
                        '</testcase></testsuite></testsuites>'
                    )
                    self.assertEqual(detected(report, code), expected)

    def test_missing_or_incomplete_report_is_not_detection(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.xml"
            self.assertFalse(detected(report, 1))
            report.write_text('<testsuites>')
            self.assertFalse(detected(report, 1))
