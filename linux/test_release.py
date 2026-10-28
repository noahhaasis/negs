"""Exercise release boundaries with a real temporary Git repo and Arch metadata tools."""
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('linux_release', HERE / 'release.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory(prefix='negs-release-test-')
        self.addCleanup(self.work.cleanup)
        self.root = Path(self.work.name) / 'repo'
        self.root.mkdir()
        for name in release.SOURCE_PATHS:
            path = self.root / name
            if '.' not in path.name and name != 'LICENSE':
                path.mkdir(parents=True, exist_ok=True)
                (path / 'fixture.txt').write_text('fixture\n')
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('fixture\n')
        (self.root / 'Cargo.toml').write_text('[workspace.package]\nversion = "0.8.0"\n')
        (self.root / 'linux/Cargo.toml').write_text('[package]\nversion = "0.8.0"\n')
        (self.root / 'linux/arch').mkdir()
        shutil.copy2(HERE / 'arch/PKGBUILD', self.root / 'linux/arch/PKGBUILD')
        for args in [('init', '-q'), ('config', 'user.name', 'Release test'),
                     ('config', 'user.email', 'test@example.invalid'), ('add', '.'),
                     ('commit', '-qm', 'fixture')]:
            release.git(*args, root=self.root)
        self.out = Path(self.work.name) / 'out'
        self.env = {'RELEASE_TAG': 'v0.8.0', 'RELEASE_REPOSITORY': 'example/negs'}

    def prepare(self):
        return release.prepare(self.out, root=self.root, env=self.env)

    def test_source_is_deterministic_and_excludes_private_untracked_files(self):
        private = self.root / 'docs/linux-agent-handoff.md'
        private.write_text('private transferred material')
        (self.root / 'target').mkdir()
        (self.root / 'target/private-photo.NEF').write_bytes(b'private')
        meta = self.prepare()
        other = Path(self.work.name) / 'other'
        release.prepare(other, root=self.root, env=self.env)
        source = self.out / 'assets/negs-0.8.0.tar.gz'
        self.assertEqual(source.read_bytes(), (other / 'assets' / source.name).read_bytes())
        with tarfile.open(source) as archive:
            names = archive.getnames()
        self.assertIn('negs-0.8.0/Cargo.toml', names)
        self.assertFalse(any('handoff' in name or 'private' in name for name in names))
        recipe = (self.out / 'build/PKGBUILD').read_text()
        self.assertIn('pkgver=0.8.0', recipe)
        self.assertIn(meta['source_sha256'], recipe)
        self.assertNotIn('SKIP', recipe)

    def test_wrong_tag_version_drift_and_dirty_source_are_rejected(self):
        with self.assertRaisesRegex(ValueError, 'Tag'):
            release.metadata(self.root, dict(self.env, RELEASE_TAG='v9.0.0'))
        (self.root / 'linux/Cargo.toml').write_text('[package]\nversion = "0.9.0"\n')
        with self.assertRaisesRegex(ValueError, 'must match'):
            self.prepare()
        release.git('restore', 'linux/Cargo.toml', root=self.root)
        (self.root / 'LICENSE').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'Commit tracked changes'):
            self.prepare()

    def test_metainfo_release_must_match_cargo(self):
        metainfo = self.root / f'linux/{release.APP_ID}.metainfo.xml'
        metainfo.write_text('<component><releases><release version="0.7.0" date="2026-01-01"/>'
                            '</releases></component>')
        with self.assertRaisesRegex(ValueError, 'newest release'):
            release.metadata(self.root, self.env)
        metainfo.write_text('<component><releases><release version="0.8.0" date="2026-01-01"/>'
                            '<release version="0.7.0" date="2025-01-01"/></releases></component>')
        self.assertEqual(release.metadata(self.root, self.env)['version'], '0.8.0')

    def test_existing_output_is_rejected(self):
        self.prepare()
        with self.assertRaisesRegex(ValueError, 'empty release output'):
            self.prepare()

    @unittest.skipUnless(shutil.which('makepkg') and shutil.which('bsdtar'), 'requires Arch packaging tools')
    def test_packages_and_downstream_recipes_refer_to_the_same_release(self):
        self.prepare()
        stage = Path(self.work.name) / 'stage'
        (stage / 'usr/bin').mkdir(parents=True)
        (stage / 'usr/bin/negs').write_text('fixture binary')
        package = self.out / 'build/negs-0.8.0-1-x86_64.pkg.tar.zst'
        subprocess.run(['bsdtar', '--zstd', '-cf', str(package), '-C', str(stage), 'usr'], check=True)
        release.finish(self.out, package, root=self.root, env=self.env)
        assets = self.out / 'assets'
        for line in (assets / 'SHA256SUMS-linux').read_text().splitlines():
            digest, name = line.split('  ')
            self.assertEqual(digest, release.sha256(assets / name))
        with tarfile.open(assets / 'negs-0.8.0-linux-x86_64.tar.gz') as archive:
            self.assertEqual(archive.extractfile('bin/negs').read(), b'fixture binary')
        recipes = self.out / 'recipes'
        source = (recipes / 'aur/negs/PKGBUILD').read_text()
        self.assertIn('url=\'https://github.com/example/negs\'', source)
        self.assertIn('$url/releases/download/v$pkgver/negs-$pkgver.tar.gz', source)
        binary = (recipes / 'aur/negs-bin/.SRCINFO').read_text()
        self.assertIn('provides = negs', binary)
        self.assertIn('conflicts = negs', binary)
        self.assertIn(release.sha256(assets / 'negs-0.8.0-linux-x86_64.tar.gz'), binary)
        omarchy = recipes / 'omarchy/pkgbuilds/negs'
        self.assertEqual((omarchy / 'PKGBUILD').read_text(), source)
        watch = json.loads((omarchy / '.omarchy/package.json').read_text())
        self.assertEqual(watch['upstream']['watch']['github'], 'example/negs')
        self.assertTrue(watch['upstream']['watch']['allow_prerelease'])
        self.assertNotIn('release_ring', watch)

    def test_modified_source_archive_is_rejected(self):
        self.prepare()
        (self.out / 'assets/negs-0.8.0.tar.gz').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'Source archive changed'):
            release.finish(self.out, self.out / 'build/negs-0.8.0-1-x86_64.pkg.tar.zst',
                           root=self.root, env=self.env)


if __name__ == '__main__':
    unittest.main()
