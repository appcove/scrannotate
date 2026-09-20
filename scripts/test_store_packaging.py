"""Packaging boundary tests using fake native tools, never real signing or SDK validation.

The PowerShell case requires pwsh on a POSIX host. It removes only the native
Windows guard in a temporary script copy so the SDK/error-handling paths can
be checked without claiming that MakeAppx ran. No external app is installed.
"""

import datetime
import os
import pathlib
import plistlib
import shutil
import struct
import subprocess
import tempfile
import textwrap
import unittest
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parents[1]


@unittest.skipUnless(os.name == "posix", "mock native executables use POSIX shebangs")
class StorePackagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="scrannotate-store-tests-")
        self.addCleanup(self.temporary.cleanup)
        base = pathlib.Path(self.temporary.name)
        root = base / "project"
        (root / "packaging/macos").mkdir(parents=True)
        (root / "packaging/windows").mkdir(parents=True)
        for name in [
            "LICENSE",
            "NOTICE",
            "PRIVACY.md",
            "packaging/macos/Info.plist",
            "packaging/macos/store.sh",
            "packaging/macos/store-entitlements.plist",
            "packaging/windows/store.ps1",
        ]:
            shutil.copyfile(ROOT / name, root / name)
        fixtures = base / "fixtures"
        fixtures.mkdir()
        notices = fixtures / "notices.txt"
        notices.write_text("verified license fixture")
        mocks = base / "tools"
        mocks.mkdir()
        self.base, self.root, self.fixtures, self.notices, self.mocks = (
            base,
            root,
            fixtures,
            notices,
            mocks,
        )

    def test_mac_package_validation_and_cleanup(self):
        base, root, fixtures, notices, mocks = (
            self.base,
            self.root,
            self.fixtures,
            self.notices,
            self.mocks,
        )
        binary = fixtures / "scrannotate"
        binary.write_bytes(
            b"SCRANNOTATE_MAC_APP_STORE_BUILD=1\0https://example.com/privacy\0"
        )
        icon = fixtures / "app.icns"
        icon.write_bytes(b"icnsfixture")
        profile = fixtures / "profile.provisionprofile"
        payload = {
            "TeamIdentifier": ["TEAM123456"],
            "Platform": ["OSX"],
            "ExpirationDate": datetime.datetime.now(datetime.timezone.utc).replace(
                tzinfo=None
            )
            + datetime.timedelta(days=1),
            "Entitlements": {
                "com.apple.developer.team-identifier": "TEAM123456",
                "com.apple.application-identifier": "TEAM123456.com.appcove.scrannotate",
            },
            "DeveloperCertificates": [b"certificate fixture"],
        }

        def profile_write():
            profile.write_bytes(plistlib.dumps(payload))

        profile_write()
        executable = """
    #!/usr/bin/env python3
    import os,pathlib,sys,shutil
    name=pathlib.Path(sys.argv[0]).name
    args=sys.argv[1:]
    with open(os.environ['STORE_TEST_LOG'],'a') as log: log.write(name+' '+repr(args)+'\\n')
    if name=='uname': print('Darwin')
    elif name=='security': shutil.copyfile(args[args.index('-i')+1],args[args.index('-o')+1])
    elif name=='lipo': print('x86_64 arm64')
    elif name=='iconutil':
        dest=pathlib.Path(args[args.index('-o')+1]);dest.mkdir();(dest/'icon_512x512@2x.png').write_bytes(b'icon')
    elif name=='codesign' and '--extract-certificates' in args:
        pathlib.Path(args[args.index('--extract-certificates')+1]+'0').write_bytes(b'certificate fixture')
    elif name=='productbuild':
        if os.environ.get('STORE_TEST_FAIL'): sys.exit(19)
        app=pathlib.Path(args[args.index('--component')+1]); assert (app/'Contents/Resources/THIRD_PARTY_NOTICES.txt').is_file()
        assert (app/'Contents/embedded.provisionprofile').is_file()
        pathlib.Path(args[-1]).write_bytes(b'mock pkg')
    """
        for name in [
            "uname",
            "security",
            "lipo",
            "iconutil",
            "codesign",
            "productbuild",
            "pkgutil",
            "plutil",
        ]:
            path = mocks / name
            path.write_text(textwrap.dedent(executable).lstrip())
            path.chmod(0o755)
        log = base / "commands.log"
        env = dict(
            os.environ,
            PATH=str(mocks) + os.pathsep + os.environ["PATH"],
            STORE_TEST_LOG=str(log),
        )
        output = base / "dist/output.pkg"
        command = [
            "bash",
            str(root / "packaging/macos/store.sh"),
            "--binary",
            str(binary),
            "--version",
            "0.4.0",
            "--build-number",
            "1",
            "--application-identity",
            "Apple Distribution: Fixture (TEAM123456)",
            "--installer-identity",
            "3rd Party Mac Developer Installer: Fixture (TEAM123456)",
            "--profile",
            str(profile),
            "--icon",
            str(icon),
            "--privacy-url",
            "https://example.com/privacy",
            "--notices",
            str(notices),
            "--output",
            str(output),
        ]

        def run_mac(expected, env_extra=None):
            result = subprocess.run(
                command,
                env=dict(env, **(env_extra or {})),
                text=True,
                capture_output=True,
            )
            if (result.returncode == 0) != expected:
                raise AssertionError(result.stdout + "\n" + result.stderr)
            assert not list(output.parent.glob(".scrannotate-store.*")), (
                "leaked staging"
            )
            return result

        run_mac(True)
        assert output.read_bytes() == b"mock pkg"
        assert "--verify" in log.read_text() and "--check-signature" in log.read_text()
        run_mac(False)
        assert output.read_bytes() == b"mock pkg"
        output.unlink()
        run_mac(False, {"STORE_TEST_FAIL": "1"})
        assert not output.exists()
        payload["ExpirationDate"] = datetime.datetime(2000, 1, 1)
        profile_write()
        run_mac(False)
        payload["ExpirationDate"] = datetime.datetime.now(
            datetime.timezone.utc
        ).replace(tzinfo=None) + datetime.timedelta(days=1)
        payload["Entitlements"]["com.apple.application-identifier"] = (
            "TEAM123456.other.app"
        )
        profile_write()
        run_mac(False)
        payload["Entitlements"]["com.apple.application-identifier"] = (
            "TEAM123456.com.appcove.scrannotate"
        )
        payload["ProvisionedDevices"] = ["fixture"]
        profile_write()
        run_mac(False)
        del payload["ProvisionedDevices"]
        profile_write()
        payload["DeveloperCertificates"] = [b"different signing certificate"]
        profile_write()
        run_mac(False)
        payload["DeveloperCertificates"] = [b"certificate fixture"]
        profile_write()
        binary.write_bytes(
            b"SCRANNOTATE_MAC_APP_STORE_BUILD=0\0https://example.com/privacy\0"
        )
        run_mac(False)
        binary.write_bytes(
            b"SCRANNOTATE_MAC_APP_STORE_BUILD=1\0https://example.com/other\0"
        )
        run_mac(False)

    @unittest.skipUnless(shutil.which("pwsh"), "PowerShell is not installed")
    def test_windows_package_validation_and_cleanup(self):
        base, root, fixtures, notices, mocks = (
            self.base,
            self.root,
            self.fixtures,
            self.notices,
            self.mocks,
        )
        # Exercise actual PowerShell control flow on Linux, replacing only the
        # platform guard in a temporary copy; MakeAppx is explicitly a fake tool.
        script = root / "packaging/windows/store.ps1"
        source = script.read_text()
        self.assertEqual(
            source.count(
                "if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT)"
            ),
            1,
            "mock must replace exactly the platform guard",
        )
        source = source.replace(
            "if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT)",
            "if ($false)",
        )
        script.write_text(source)
        pe = fixtures / "scrannotate.exe"
        data = bytearray(256)
        struct.pack_into("<H", data, 0, 0x5A4D)
        struct.pack_into("<I", data, 0x3C, 0x80)
        struct.pack_into("<IH", data, 0x80, 0x4550, 0x8664)
        pe.write_bytes(data)
        assets = fixtures / "assets"
        assets.mkdir()
        for name, size in [
            ("StoreLogo.png", 50),
            ("Square44x44Logo.png", 44),
            ("Square150x150Logo.png", 150),
        ]:
            (assets / name).write_bytes(
                b"\x89PNG\r\n\x1a\n"
                + struct.pack(">I", 13)
                + b"IHDR"
                + struct.pack(">II", size, size)
            )
        sdk = mocks / "makeappx.exe"
        sdk.write_text(
            textwrap.dedent("""
    #!/usr/bin/env python3
    import os,pathlib,sys,shutil
    args=sys.argv[1:]
    assert args[0]=='pack' and '/no' in args and '/nv' not in args
    payload=pathlib.Path(args[args.index('/d')+1])
    for name in ['LICENSE','NOTICE','PRIVACY.md','THIRD_PARTY_NOTICES.txt','scrannotate.exe']: assert (payload/name).is_file()
    shutil.copyfile(payload/'AppxManifest.xml',os.environ['STORE_TEST_MANIFEST'])
    if os.environ.get('STORE_TEST_FAIL'): sys.exit(17)
    pathlib.Path(args[args.index('/p')+1]).write_bytes(b'mock msix')
    """).lstrip()
        )
        sdk.chmod(0o755)
        manifest = base / "AppxManifest.xml"
        tmpdir = base / "ps-tmp"
        tmpdir.mkdir()
        psenv = dict(os.environ, STORE_TEST_MANIFEST=str(manifest), TMPDIR=str(tmpdir))
        winout = base / "dist/output.msix"
        wincommand = [
            shutil.which("pwsh"),
            "-NoProfile",
            "-File",
            str(script),
            "-Binary",
            str(pe),
            "-Version",
            "1.0.400.0",
            "-Architecture",
            "x64",
            "-IdentityName",
            "Publisher.scrannotate",
            "-Publisher",
            'CN=Publisher & "Example"',
            "-PublisherDisplayName",
            "Publisher's & Co",
            "-AssetsDirectory",
            str(assets),
            "-Notices",
            str(notices),
            "-MakeAppxPath",
            str(sdk),
            "-MaxVersionTested",
            "10.0.26100.0",
            "-Output",
            str(winout),
        ]

        def run_win(expected, replace=None, extra=None):
            cmd = list(wincommand)
            for key, value in (replace or {}).items():
                cmd[cmd.index(key) + 1] = value
            result = subprocess.run(
                cmd, env=dict(psenv, **(extra or {})), text=True, capture_output=True
            )
            if (result.returncode == 0) != expected:
                raise AssertionError(result.stdout + "\n" + result.stderr)
            assert not list(tmpdir.glob("scrannotate-msix-*")), "leaked staging"
            return result

        run_win(True)
        assert winout.read_bytes() == b"mock msix"
        xml = ET.parse(manifest)
        ns = {
            "a": "http://schemas.microsoft.com/appx/manifest/foundation/windows10",
            "u": "http://schemas.microsoft.com/appx/manifest/uap/windows10/5",
        }
        assert (
            xml.find("a:Identity", ns).attrib["Publisher"] == 'CN=Publisher & "Example"'
        )
        assert xml.find(".//u:ExecutionAlias", ns).attrib["Alias"] == "scrannotate.exe"
        run_win(False)
        assert winout.read_bytes() == b"mock msix"
        winout.unlink()
        run_win(False, extra={"STORE_TEST_FAIL": "1"})
        assert not winout.exists()
        for version in [
            "0.4.0.0",
            "1.0.0.1",
            "1.0.70000.0",
            "1.2.3",
            "01.2.3.0",
            "1.2.3-beta.0",
        ]:
            run_win(False, {"-Version": version})
        run_win(False, {"-Architecture": "arm64"})
        run_win(False, {"-MaxVersionTested": "10.0.10240.0"})
        run_win(False, {"-Publisher": "CN=Invalid\x01Publisher"})
        (assets / "StoreLogo.png").write_bytes(b"not a PNG")
        run_win(False)


if __name__ == "__main__":
    unittest.main()
