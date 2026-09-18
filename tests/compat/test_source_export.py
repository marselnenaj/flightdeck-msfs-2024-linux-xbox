# SPDX-License-Identifier: MIT
import importlib.util
import base64
import contextlib
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("source_check", ROOT / "scripts/check-source-export.py")
source_check = importlib.util.module_from_spec(spec); spec.loader.exec_module(source_check)
spec = importlib.util.spec_from_file_location("source_release", ROOT / "scripts/source-release.py")
source_release = importlib.util.module_from_spec(spec); spec.loader.exec_module(source_release)


class SourceExport(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.previous = source_check.ROOT
        source_check.ROOT = self.root = Path(self.tmp.name)
        (self.root / "scripts").mkdir()
        (self.root / "scripts/example.py").write_text("print('example')\n")

    def tearDown(self):
        source_check.ROOT = self.previous
        self.tmp.cleanup()

    def test_only_explicit_source_paths(self):
        for directory in ("private", "build", "games", "runtime"):
            (self.root / directory).mkdir()
            (self.root / directory / "secret.txt").write_text("not for release")
        self.assertEqual(list(source_check.inspect()["files"]), ["scripts/example.py"])

    def test_binary_extension_and_magic_rejected(self):
        bad = self.root / "scripts/bad.dll"; bad.write_bytes(b"dummy")
        with self.assertRaisesRegex(ValueError, "Unrecognized"):
            source_check.inspect()
        bad.unlink(); (self.root / "scripts/bad.py").write_bytes(b"MZfake")
        with self.assertRaisesRegex(ValueError, "Binary"):
            source_check.inspect()

    def test_only_exact_reviewed_artwork_is_allowed(self):
        asset = self.root / "ui/flight-panorama.png"
        asset.parent.mkdir()
        original = ROOT / "ui/flight-panorama.png"
        asset.write_bytes(original.read_bytes())
        self.assertIn("ui/flight-panorama.png", source_check.inspect()["files"])
        asset.write_bytes(b"replacement private image")
        with self.assertRaisesRegex(ValueError, "Unreviewed artwork"):
            source_check.inspect()
        asset.unlink()
        (asset.parent / "private-screenshot.png").write_bytes(b"private")
        with self.assertRaisesRegex(ValueError, "Unrecognized"):
            source_check.inspect()

    def test_symlink_and_personal_path_rejected(self):
        link = self.root / "scripts/link.py"; link.symlink_to(self.root / "scripts/example.py")
        with self.assertRaisesRegex(ValueError, "symlinks"):
            source_check.inspect()
        link.unlink()
        # Assemble to keep this regression source itself free of machine paths.
        (self.root / "scripts/bad.py").write_text("/" + "home/" + "synthetic-user/" + "secret")
        with self.assertRaisesRegex(ValueError, "Personal absolute"):
            source_check.inspect()

    def test_all_top_level_symlinks_rejected_including_dangling(self):
        for name in source_check.TOP_LEVEL:
            for target in (self.root / "scripts/example.py", self.root / "missing"):
                with self.subTest(name=name, dangling=not target.exists()):
                    link = self.root / name
                    link.symlink_to(target)
                    try:
                        with self.assertRaisesRegex(ValueError, "symlinks"):
                            source_check.inspect()
                    finally:
                        link.unlink()

    def test_credential_account_and_browser_files_rejected(self):
        for name in (".env", ".env.production", "tokens.json", "account.json", "accounts.json",
                     "credentials.yaml", "xodus-keyring.ron", "msal_token_cache.json", ".netrc",
                     "cookies.json", "storage-state.json", "Local Storage/entry.json", "private/run.json"):
            with self.subTest(name=name):
                path = self.root / "scripts" / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("{}")
                try:
                    with self.assertRaisesRegex(ValueError, "Private data file"):
                        source_check.inspect()
                finally:
                    path.unlink()

    def test_templates_auth_code_project_links_and_licenses_allowed(self):
        for name in (".env.example", ".env.sample", ".env.template"):
            (self.root / "scripts" / name).write_text("ACCESS_TOKEN=replace-me\nREGION=US\n")
        safe = '''access_token = get_token()
refresh_token = "synthetic-placeholder"
authorization = "Bearer ${TOKEN}"
example = "eyJTest.Exact_Payload.Signature-123"
https://github.com/example-org/example-project
Copyright Example Contributors <maintainer@example.invalid>
'''
        (self.root / "scripts/store_account.py").write_text(safe)
        (self.root / "scripts/token-parser.py").write_text(safe)
        self.assertEqual(len(source_check.inspect()["files"]), 6)
        # Template names cannot bypass the content scan.
        value = "A9b8C7d6E5f4G3h2I1j0K9l8M7n6O5p4"
        (self.root / "scripts/.env.example").write_text('access_token="' + value + '"')
        with self.assertRaisesRegex(ValueError, "credential literal"):
            source_check.inspect()

    def test_private_key_variants_embedded_and_patch_form(self):
        body = base64.b64encode(bytes(range(96))).decode()
        path = self.root / "scripts/fixture.txt"
        for label in ("PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY", "DSA PRIVATE KEY",
                      "OPENSSH PRIVATE KEY", "ENCRYPTED PRIVATE KEY", "PGP PRIVATE KEY BLOCK"):
            block = "-----" + "BEGIN " + label + "-----\n" + body + "\n-----END " + label + "-----"
            for value in (block, json.dumps({"key": block}), "+" + block.replace("\n", "\n+")):
                with self.subTest(label=label, encoded=value.startswith("{")):
                    path.write_text(value)
                    with self.assertRaisesRegex(ValueError, "Private key material") as caught:
                        source_check.inspect()
                    self.assertNotIn(body, str(caught.exception))
        # A header mention or a deliberately redacted documentation block is harmless.
        path.write_text("-----" + "BEGIN PRIVATE KEY-----\n<redacted>\n-----END PRIVATE KEY-----")
        source_check.inspect()

    def test_structured_signed_token_rejected_without_claim_output(self):
        def segment(value):
            return base64.urlsafe_b64encode(json.dumps(value).encode()).decode().rstrip("=")
        signature = base64.urlsafe_b64encode(bytes(range(64))).decode().rstrip("=")
        header = segment({"alg": "RS256", "typ": "JWT"})
        payload = segment({"sub": "synthetic-subject", "exp": 1})
        value = ".".join((header, payload, signature))
        path = self.root / "scripts/fixture.txt"
        path.write_text(value)
        with self.assertRaisesRegex(ValueError, "signed-token data") as caught:
            source_check.inspect()
        self.assertNotIn(value, str(caught.exception))
        self.assertNotIn("synthetic-subject", str(caught.exception))
        for value in ("eyJTest.Exact_Payload.Signature-123",
                      ".".join((segment({"alg": "none"}), payload, signature)),
                      ".".join((segment({"alg": ["RS256"]}), payload, signature)),
                      ".".join((header, segment("not an object"), signature)),
                      ".".join((header, "not-valid-json", signature))):
            with self.subTest(value=value[:20]):
                path.write_text(value)
                source_check.inspect()

    def test_private_jwk_rejected_public_and_placeholder_keys_allowed(self):
        material = base64.urlsafe_b64encode(bytes(range(32))).decode().rstrip("=")
        path = self.root / "scripts/fixture.json"
        for kind, field in (("RSA", "d"), ("EC", "d"), ("OKP", "d"), ("oct", "k")):
            with self.subTest(kind=kind):
                path.write_text(json.dumps({"keys": [{"kty": kind, field: material}]}))
                with self.assertRaisesRegex(ValueError, "Private JWK material"):
                    source_check.inspect()
        for item in ({"kty": "RSA", "n": material, "e": "AQAB"},
                     {"kty": "EC", "x": material, "y": material},
                     {"kty": "RSA", "d": "<private component>"},
                     {"kty": ["RSA", "EC"], "d": "schema field"},
                     {"schema": {"kty": "string", "d": "string"}}):
            path.write_text(json.dumps(item))
            source_check.inspect()

    def test_access_keys_and_credential_literals_rejected_without_values(self):
        material = "A9b8C7d6E5f4G3h2I1j0K9l8M7n6O5p4Q3r2S1t0U9v8W7x6Y5z4"
        path = self.root / "scripts/fixture.txt"
        for value in ("ghp_" + material, "github_pat_" + material * 2, "sk-proj-" + material,
                      'refresh_token="' + material + '"', '{"client_secret":"' + material + '"}',
                      "Bearer " + material, "XBL3.0 x=" + "1234567890;" + material):
            with self.subTest(kind=value[:8]):
                path.write_text(value)
                with self.assertRaisesRegex(ValueError, "Embedded") as caught:
                    source_check.inspect()
                self.assertNotIn(material, str(caught.exception))
        for value in ('access_token = "' + "A" * 80 + '"', "Bearer " + "0" * 80,
                      'client_secret="${CLIENT_SECRET}"', 'XBL3.0 x={hash};{token}'):
            path.write_text(value)
            source_check.inspect()


class SourceRelease(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.source = self.root / "scripts/example.py"
        self.source.parent.mkdir()
        self.source.write_text("print('inspected bytes')\n")
        self.output = self.root / "build/source.tar.gz"
        self.patches = [mock.patch.object(source_release, "ROOT", self.root),
                        mock.patch.object(source_release.check, "ROOT", self.root)]
        for patch in self.patches:
            patch.start()

    def tearDown(self):
        for patch in reversed(self.patches):
            patch.stop()
        self.tmp.cleanup()

    def create(self):
        with contextlib.redirect_stdout(io.StringIO()):
            source_release.create(self.output)

    def test_changed_source_rejected_before_output_creation(self):
        manifest = source_release.check.inspect()
        self.source.write_text("print('changed after inspection')\n")
        with mock.patch.object(source_release.check, "inspect", return_value=manifest):
            with self.assertRaisesRegex(ValueError, "Source changed after inspection"):
                self.create()
        self.assertFalse(self.output.parent.exists())

    def test_disappeared_source_rejected_before_output_creation(self):
        manifest = source_release.check.inspect()
        self.source.unlink()
        with mock.patch.object(source_release.check, "inspect", return_value=manifest):
            with self.assertRaises(FileNotFoundError):
                self.create()
        self.assertFalse(self.output.parent.exists())

    def test_archive_uses_frozen_bytes_and_matches_manifest(self):
        inspected = self.source.read_bytes()
        real_open = tarfile.open

        def change_source_when_archive_opens(*args, **kwargs):
            self.source.write_text("print('changed after preflight')\n")
            return real_open(*args, **kwargs)

        with mock.patch.object(source_release.tarfile, "open", side_effect=change_source_when_archive_opens):
            self.create()
        with tarfile.open(self.output, "r:gz") as archive:
            manifest = json.load(archive.extractfile("flightdeck-linux/SOURCE-MANIFEST.json"))
            self.assertEqual(set(archive.getnames()),
                             {"flightdeck-linux/" + name for name in manifest["files"]} |
                             {"flightdeck-linux/SOURCE-MANIFEST.json"})
            for name, expected in manifest["files"].items():
                data = archive.extractfile("flightdeck-linux/" + name).read()
                self.assertEqual(hashlib.sha256(data).hexdigest(), expected)
            self.assertEqual(archive.extractfile("flightdeck-linux/scripts/example.py").read(), inspected)

    def test_existing_archive_remains_untouched(self):
        self.output.parent.mkdir()
        self.output.write_bytes(b"existing source archive")
        with self.assertRaisesRegex(ValueError, "Output exists"):
            self.create()
        self.assertEqual(self.output.read_bytes(), b"existing source archive")


if __name__ == "__main__":
    unittest.main()
