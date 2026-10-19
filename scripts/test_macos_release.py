import base64
from pathlib import Path
import plistlib
import tempfile
import unittest
import zipfile

from macos_release import metadata, write_plist
from prepare_macos_update import check_archive_configuration, check_previous_feed, SPARKLE


class ReleaseTests(unittest.TestCase):
    def test_tag_and_update_configuration_fail_closed(self):
        version = metadata(env={})["version"]
        with self.assertRaises(ValueError):
            metadata(env={"RELEASE_TAG": "v9.9.9"})
        with self.assertRaises(ValueError):
            metadata(env={"RELEASE_TAG": f"v{version}"})
        with self.assertRaises(ValueError):
            metadata(env={"UPDATE_FEED_URL": "http://example.com/appcast.xml", "SPARKLE_PUBLIC_KEY": "bad"})

    def test_packaged_version_and_update_policy(self):
        key = base64.b64encode(bytes(range(32))).decode()
        meta = metadata(env={"RELEASE_REPOSITORY": "owner/spektrafilm", "SPARKLE_PUBLIC_KEY": key,
                             "RELEASE_TAG": f"v{metadata(env={})['version']}"})
        self.assertEqual(meta["feed"], "https://owner.github.io/spektrafilm/appcast.xml")
        with tempfile.TemporaryDirectory() as temp:
            file = Path(temp) / "Info.plist"
            write_plist(file, meta)
            info = plistlib.loads(file.read_bytes())
        self.assertEqual(info["CFBundleShortVersionString"], meta["version"])
        self.assertEqual(info["CFBundleVersion"], meta["build"])
        self.assertTrue(info["SURequireSignedFeed"])
        self.assertTrue(info["SUVerifyUpdateBeforeExtraction"])
        self.assertFalse(info["SUAllowsAutomaticUpdates"])

    def test_existing_feed_requires_a_new_build(self):
        feed = f'<rss xmlns:sparkle="{SPARKLE}"><channel><item><sparkle:version>42</sparkle:version></item></channel></rss>'
        check_previous_feed(feed, "43")
        for value in ["1", "41", "42"]:
            with self.assertRaises(ValueError):
                check_previous_feed(feed, value)

    def test_archive_must_contain_the_configured_key_and_version(self):
        key = base64.b64encode(bytes(range(32))).decode()
        meta = metadata(env={"RELEASE_REPOSITORY": "owner/spektrafilm", "SPARKLE_PUBLIC_KEY": key})
        with tempfile.TemporaryDirectory() as temp:
            file = Path(temp) / "Info.plist"
            archive = Path(temp) / "app.zip"
            write_plist(file, meta)
            with zipfile.ZipFile(archive, "w") as zipped:
                zipped.write(file, "Spektrafilm.app/Contents/Info.plist")
            check_archive_configuration(archive, meta)
            for field, value in [("public_key", base64.b64encode(bytes(32)).decode()), ("build", "999"), ("feed", "https://wrong.example/feed.xml")]:
                with self.assertRaises(ValueError):
                    check_archive_configuration(archive, dict(meta, **{field: value}))


if __name__ == "__main__":
    unittest.main()
