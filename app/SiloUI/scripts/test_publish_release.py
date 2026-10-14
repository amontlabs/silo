import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import os
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('publish', Path(__file__).with_name('publish-release.py'))
publish = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish)

class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        for name in publish.EXPECTED:
            (self.root/name).write_text('signedfixture' if name.endswith('.sig') else 'package')

    def test_macos_release_has_version_pinned_feed_and_every_checksum(self):
        publish.prepare(self.root, '0.1.0', 'test/repo', 'Release notes')
        feed = json.loads((self.root/'latest.json').read_text())
        self.assertEqual(feed['version'], '0.1.0')
        self.assertEqual(set(feed['platforms']), set(publish.PLATFORMS))
        for target, name in publish.PLATFORMS.items():
            self.assertEqual(feed['platforms'][target]['url'], f'https://github.com/test/repo/releases/download/v0.1.0/{name}')
        names = {line.split('  ')[1] for line in (self.root/'SHA256SUMS').read_text().splitlines()}
        self.assertEqual(names, publish.EXPECTED|{'latest.json'})

    def test_release_page_lists_downloads_first_and_collapses_change_lists(self):
        notes = '# Silo 0.2.0\n\n### Highlights\n\n- **Big.** Thing.\n\n### Minor Changes\n\n- abc1234: Feature.\n\n  More detail.\n\n### Patch Changes\n\n- def5678: Fix.\n'
        page = publish.release_page('0.2.0', 'test/repo', notes)
        self.assertTrue(page.startswith('| Download | |'))
        self.assertNotIn('# Silo', page)
        for name in publish.PACKAGES - set(publish.PLATFORMS.values()):
            self.assertIn(f'(https://github.com/test/repo/releases/download/v0.2.0/{name})', page)
        self.assertLess(page.index('### Highlights'), page.index('<details>'))
        self.assertIn('<summary>All changes (2)</summary>', page)
        self.assertLess(page.index('<details>'), page.index('### Minor Changes'))
        self.assertLess(page.index('### Patch Changes'), page.index('</details>'))

    def test_release_page_without_highlights_still_collapses(self):
        page = publish.release_page('0.2.0', 'test/repo', '# Silo 0.2.0\n\n### Patch Changes\n\n- def5678: Fix.\n')
        self.assertIn('\n\n<details>\n<summary>All changes (1)</summary>', page)

    def test_draft_page_is_separate_from_feed_notes(self):
        with tempfile.TemporaryDirectory() as directory:
            cwd = os.getcwd(); os.chdir(directory); self.addCleanup(os.chdir, cwd)
            assets = Path('release-assets'); assets.mkdir()
            for name in publish.EXPECTED:
                (assets/name).write_text('signedfixture' if name.endswith('.sig') else 'package')
            Path('release-notes.md').write_text('# Silo 0.1.0\n\n### Patch Changes\n\n- abc: Fix.\n')
            with patch.dict(os.environ,GH_REPO='test/repo',GITHUB_SHA='sha'), patch('sys.argv',['publish','0.1.0']), patch.object(publish,'gh',side_effect=['[]','sha','']) as gh:
                publish.main()
            create = gh.call_args_list[-1].args
            self.assertEqual(create[create.index('--notes-file')+1], 'release-page.md')
            self.assertTrue(Path('release-page.md').read_text().startswith('| Download | |'))
            self.assertEqual(json.loads((assets/'latest.json').read_text())['notes'], Path('release-notes.md').read_text())

    def test_missing_updater_archive_fails_before_feed(self):
        (self.root/'Silo-macos-arm64.app.tar.gz').unlink()
        with self.assertRaises(RuntimeError): publish.prepare(self.root,'0.1.0','test/repo','notes')
        self.assertFalse((self.root/'latest.json').exists())

    def test_empty_package_rejected(self):
        (self.root/'Silo-macos-arm64.dmg').write_text('')
        with self.assertRaises(RuntimeError): publish.prepare(self.root,'0.1.0','test/repo','notes')

    def test_unexpected_file_rejected(self):
        (self.root/'private.key').write_text('never upload')
        with self.assertRaises(RuntimeError): publish.prepare(self.root,'0.1.0','test/repo','notes')

    def test_symlink_rejected(self):
        name=self.root/'Silo-macos-arm64.dmg';name.unlink();name.symlink_to(self.root/'Silo-macos-arm64.app.tar.gz')
        with self.assertRaises(RuntimeError): publish.prepare(self.root,'0.1.0','test/repo','notes')

    def test_bad_signature_rejected(self):
        (self.root/'Silo-macos-arm64.app.tar.gz.sig').write_text('not a signature')
        with self.assertRaises(RuntimeError): publish.prepare(self.root,'0.1.0','test/repo','notes')

    def test_only_stable_nonzero_versions(self):
        for version in ['0.0.0','01.2.3','1.0','1.0.0-beta','1.0.0/other','v1.0.0']:
            with self.subTest(version=version), self.assertRaises(RuntimeError): publish.validate_version(version)
        self.assertEqual(publish.validate_version('1.2.10'), (1,2,10))

    def test_stable_release_requires_explicit_opt_in_before_github_calls(self):
        for version in ('1.0.0', '2.3.4'):
            for action in ([], ['--publish']):
                with self.subTest(version=version, action=action), \
                        patch.dict(os.environ, GH_REPO='test/repo'), \
                        patch('sys.argv', ['publish', version, *action]), \
                        patch.object(publish, 'gh', side_effect=AssertionError('Unexpected GitHub call')) as gh:
                    with self.assertRaisesRegex(RuntimeError, 'below 1.0.0.*--allow-stable'):
                        publish.main()
                    gh.assert_not_called()

    def test_explicit_stable_opt_in_reaches_ordinary_publication_gates(self):
        with patch.dict(os.environ, GH_REPO='test/repo'), \
                patch('sys.argv', ['publish', '1.0.0', '--publish', '--allow-stable']), \
                patch.object(publish, 'gh', return_value='[]') as gh:
            with self.assertRaisesRegex(RuntimeError, 'verified draft'):
                publish.main()
            gh.assert_called_once()

    def test_existing_public_version_cannot_be_replaced(self):
        releases=[[{'tag_name':'v0.1.0','draft':False,'prerelease':False}]]
        with patch.dict(os.environ,GH_REPO='test/repo'), patch('sys.argv',['publish','0.1.0']), patch.object(publish,'gh',return_value=json.dumps(releases)) as gh:
            with self.assertRaisesRegex(RuntimeError,'newer'): publish.main()
            self.assertEqual(gh.call_count,1)

    def test_existing_draft_cannot_be_overwritten(self):
        releases=[[{'tag_name':'v0.1.0','draft':True,'prerelease':False}]]
        with patch.dict(os.environ,GH_REPO='test/repo'), patch('sys.argv',['publish','0.1.0']), patch.object(publish,'gh',return_value=json.dumps(releases)) as gh:
            with self.assertRaisesRegex(RuntimeError,'already exists'): publish.main()
            self.assertEqual(gh.call_count,1)

    def test_incomplete_draft_cannot_publish(self):
        releases=[[{'tag_name':'v0.1.0','draft':True,'prerelease':False,'assets':[]}]]
        with patch.dict(os.environ,GH_REPO='test/repo'), patch('sys.argv',['publish','0.1.0','--publish']), patch.object(publish,'gh',return_value=json.dumps(releases)) as gh:
            with self.assertRaisesRegex(RuntimeError,'incomplete'): publish.main()
            self.assertEqual(gh.call_count,1)

    def test_tag_must_match_build(self):
        with patch.dict(os.environ,GH_REPO='test/repo',GITHUB_SHA='built-sha'), patch('sys.argv',['publish','0.1.0']), patch.object(publish,'gh',side_effect=['[]','other-sha']) as gh:
            with self.assertRaisesRegex(RuntimeError,'tag'): publish.main()
            self.assertEqual(gh.call_count,2)

if __name__=='__main__': unittest.main()
