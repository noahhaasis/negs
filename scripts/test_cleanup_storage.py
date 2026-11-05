import io
from contextlib import redirect_stdout
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import cleanup_storage


class StorageCleanupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "checkout"
        self.root.mkdir()
        self.root_patch = patch.object(cleanup_storage, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        self.disposable = [
            "target/debug/incremental/session/cache",
            "target/macos-app/verify.abc/unpacked/app",
            "target/macos-app/dmg.abc/app",
            "target/macos-app/package.abc/unpacked/app",
            "target/memory-study/before.tiff",
            "target/speed-study/after.tif",
        ]
        self.preserved = [
            "target/debug/deps/library.rlib",
            "target/release/helper",
            "target/memory-study/input.dng",
            "target/memory-study/baseline-src/main.rs",
            "target/memory-study/results.json",
            "target/speed-study/prototype.patch",
            "target/speed-study/measurements.csv",
            "target/speed-study/profile.log",
            "dist/releases/app.zip",
            ".claude/worktrees/other/target/debug/incremental/cache",
        ]
        for name in self.disposable + self.preserved:
            self.write(name)

    def write(self, name):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"artifact")
        return path

    def cleanup(self, *args):
        with redirect_stdout(io.StringIO()):
            cleanup_storage.main(list(args))

    def test_dry_run_preserves_every_file(self):
        self.cleanup()
        for name in self.disposable + self.preserved:
            self.assertTrue((self.root / name).is_file(), name)

    def test_apply_removes_only_disposable_outputs(self):
        self.cleanup("--apply")
        for name in self.disposable:
            self.assertFalse((self.root / name).exists(), name)
        for name in self.preserved:
            self.assertTrue((self.root / name).is_file(), name)

    def test_full_debug_removal_is_opt_in(self):
        self.cleanup("--apply", "--debug")
        self.assertFalse((self.root / "target/debug").exists())
        self.assertTrue((self.root / "target/release/helper").is_file())

    def test_symlinked_candidates_and_children_do_not_delete_external_files(self):
        outside = Path(self.temp.name) / "outside"
        outside.mkdir()
        source = outside / "source.tiff"
        source.write_bytes(b"keep")
        (self.root / "target/memory-study/link.tiff").symlink_to(source)
        (self.root / "target/macos-app/verify.link").symlink_to(outside, target_is_directory=True)
        (self.root / "target/macos-app/verify.abc/link").symlink_to(outside, target_is_directory=True)
        self.cleanup("--apply")
        self.assertEqual(source.read_bytes(), b"keep")
        self.assertTrue((self.root / "target/memory-study/link.tiff").is_symlink())
        self.assertTrue((self.root / "target/macos-app/verify.link").is_symlink())

    def test_symlinked_build_ancestor_is_skipped(self):
        outside = Path(self.temp.name) / "outside"
        (outside / "incremental").mkdir(parents=True)
        (outside / "incremental/cache").write_bytes(b"keep")
        link_root = Path(self.temp.name) / "linked-checkout"
        (link_root / "target").mkdir(parents=True)
        (link_root / "target/debug").symlink_to(outside, target_is_directory=True)
        self.assertEqual(cleanup_storage.candidates(link_root), [])
        self.assertEqual(cleanup_storage.candidates(link_root, debug=True), [])
        self.assertEqual((outside / "incremental/cache").read_bytes(), b"keep")

    def test_sweep_is_opt_in_and_dry_by_default(self):
        with patch.object(cleanup_storage, "has_cargo_sweep", return_value=True), \
                patch.object(cleanup_storage.subprocess, "run") as run:
            self.cleanup()
            run.assert_not_called()
            self.cleanup("--sweep")
            run.assert_called_once_with(
                ["cargo", "sweep", "--time", "1", "--dry-run", str(self.root)], check=True)

    def test_sweep_apply_uses_given_days(self):
        with patch.object(cleanup_storage, "has_cargo_sweep", return_value=True), \
                patch.object(cleanup_storage.subprocess, "run") as run:
            self.cleanup("--apply", "--sweep", "7")
            run.assert_called_once_with(
                ["cargo", "sweep", "--time", "7", str(self.root)], check=True)

    def test_sweep_without_cargo_sweep_only_prints_a_hint(self):
        with patch.object(cleanup_storage, "has_cargo_sweep", return_value=False), \
                patch.object(cleanup_storage.subprocess, "run") as run:
            self.cleanup("--apply", "--sweep")
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
