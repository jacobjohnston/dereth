#!/usr/bin/env python3
"""Build Dereth.app and the Empyrean server, and sign them with a personal Apple certificate.

    python3 build.py           check, build, sign and verify both
    python3 build.py --check   check only: tools, MoltenVK, certificate, git, Dropbox
    python3 build.py --allow-dirty    build uncommitted changes too
    python3 build.py --empyrean-only  build the server alone
    python3 build.py --dereth-only    build the app alone

Settings live in build.ini beside this file. The packaging itself is the project's own
`cargo xtask package launcher`, `cargo xtask package dereth` (around that launcher) and
`cargo xtask package empyrean`; this script supplies their environment and checks the result.
The launcher carries the AC2 retail client's friends library, which only builds on Windows, so it
is taken from upstream's launcher release of the same version. Dereth.app is the whole launcher, which asks for your game data
and your art packs in its Library. `python3 local.py` runs what this builds.
"""

import argparse
import configparser
import hashlib
import io
import os
import re
import shutil
import subprocess
import sys
import tarfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CONFIG = ROOT / "build.ini"

# Folders cargo and the packager write into, kept out of Dropbox sync.
BUILD_DIRS = ["target"]
# Where the packager writes its archives, before they are unpacked into out_dir.
STAGE = "target/build-py-package"
# Where friends libraries taken from upstream's launcher releases are kept, one per version.
FRIENDS_CACHE = "target/build-py-friends"
FRIENDS_FILE = "auberean_friends.dll"
# The repository whose launcher releases carry a friends library built on Windows.
LAUNCHER_RELEASES = "https://github.com/dereth-network/dereth/releases/download"
# The programs in an Empyrean package, each signed here.
EMPYREAN_PROGRAMS = ["empyrean-server", "empyrean-import"]
# Dropbox on macOS File Provider skips anything carrying this attribute.
DROPBOX_IGNORE = "com.apple.fileprovider.ignore#P"
# Tauri notarises, or imports a certificate, when these are set; a local build does neither.
CLEARED_ENV = [
    "APPLE_ID",
    "APPLE_PASSWORD",
    "APPLE_TEAM_ID",
    "APPLE_API_KEY",
    "APPLE_API_ISSUER",
    "APPLE_API_KEY_PATH",
    "APPLE_CERTIFICATE",
    "APPLE_CERTIFICATE_PASSWORD",
    "DERETH_UPDATE_URL",
]


class Failed(Exception):
    pass


def resolve(path):
    p = Path(os.path.expanduser(path.strip()))
    return p if p.is_absolute() else ROOT / p


def load_config():
    if not CONFIG.is_file():
        raise Failed(f"no {CONFIG.name} beside {Path(__file__).name}")
    cfg = configparser.ConfigParser()
    cfg.read(CONFIG)
    try:
        return {
            "identity": cfg["signing"]["identity"].strip(),
            "source_url": cfg["build"]["source_url"].strip(),
            "moltenvk_dir": resolve(cfg["build"]["moltenvk_dir"]),
            "data_dir": resolve(cfg["build"]["data_dir"]),
            "out_dir": resolve(cfg["build"]["out_dir"]),
            "path_prepend": [
                str(resolve(p)) for p in cfg["build"].get("path_prepend", "").split(",") if p.strip()
            ],
            "applications_dir": cfg["install"].get("applications_dir", "").strip(),
            "friends_dll": (
                resolve(cfg["build"]["friends_dll"]) if cfg["build"].get("friends_dll", "").strip() else None
            ),
        }
    except KeyError as e:
        raise Failed(f"{CONFIG.name} is missing {e}") from None


def build_env(cfg):
    env = dict(os.environ)
    for name in CLEARED_ENV:
        env.pop(name, None)
    env["PATH"] = os.pathsep.join(cfg["path_prepend"] + [env.get("PATH", "")])
    env["APPLE_SIGNING_IDENTITY"] = cfg["identity"]
    env["DERETH_BUILD_SOURCE_URL"] = cfg["source_url"]
    env["EMPYREAN_BUILD_SOURCE_URL"] = cfg["source_url"]
    env["DERETH_MOLTENVK_DIR"] = str(cfg["moltenvk_dir"])
    return env


def run(args, env, capture=True):
    """Run `args` in the project folder; its output, or None when it fails or is missing."""
    try:
        out = subprocess.run(
            args, cwd=ROOT, env=env, text=True, capture_output=capture, check=False
        )
    except FileNotFoundError:
        return None
    if out.returncode != 0:
        return None
    return out.stdout if capture else ""


def version_tuple(text):
    m = re.search(r"(\d+)\.(\d+)", text or "")
    return (int(m.group(1)), int(m.group(2))) if m else None


def check(cfg, env, allow_dirty):
    """Every problem found, as lines to show; empty when the build can start."""
    problems = []

    needed = version_tuple(
        re.search(r'^rust-version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M).group(1)
    )
    rustc = run(["rustc", "--version"], env)
    have = version_tuple(rustc)
    if have is None:
        problems.append("Rust is not installed: brew install rustup && rustup default stable")
    elif have < needed:
        problems.append(f"Rust {have[0]}.{have[1]} is older than {needed[0]}.{needed[1]}: rustup update")
    if run(["cargo", "tauri", "--version"], env) is None:
        problems.append("no Tauri CLI: cargo install --locked tauri-cli --version ^2")
    if run(["cargo", "about", "--version"], env) is None:
        problems.append("no cargo-about: cargo install --locked cargo-about --features cli")
    if run(["xcrun", "--find", "codesign"], env) is None:
        problems.append("no Xcode command-line tools: xcode-select --install")

    mvk = cfg["moltenvk_dir"]
    if not any(
        (base / "MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib").is_file()
        for base in (mvk, mvk / "MoltenVK")
    ):
        problems.append(f"no unpacked MoltenVK release in {mvk}")

    identities = run(["security", "find-identity", "-v", "-p", "codesigning"], env) or ""
    if cfg["identity"].upper() not in identities.upper():
        problems.append(f"the signing certificate {cfg['identity']} is not in the keychain")

    if run(["git", "rev-parse", "--is-shallow-repository"], env) is None:
        problems.append(f"{ROOT} is not a git checkout")
    else:
        dirty = run(["git", "status", "--porcelain", "--untracked-files=no"], env)
        if dirty and not allow_dirty:
            problems.append("uncommitted changes to tracked files (the packager refuses them):")
            problems.extend("    " + line for line in dirty.splitlines()[:10])

    if not cfg["source_url"].startswith("https://github.com/"):
        problems.append("source_url must be a https://github.com/<owner>/<name> address")
    return problems


def keep_out_of_dropbox(paths):
    """Mark each folder ignored by Dropbox, making it first so nothing in it syncs."""
    for p in paths:
        p.mkdir(parents=True, exist_ok=True)
        subprocess.run(["xattr", "-w", DROPBOX_IGNORE, "1", str(p)], check=True)


def unpack_outputs(stage, out):
    """Unpack the packager's archives from `stage` into `out`, replacing earlier ones."""
    out.mkdir(parents=True, exist_ok=True)
    unpacked = []
    for archive in sorted(stage.glob("*.tar.gz")):
        with tarfile.open(archive) as tar:
            tops = {Path(name).parts[0] for name in tar.getnames()}
            for top in tops:
                if (out / top).exists():
                    shutil.rmtree(out / top)
            tar.extractall(out)
        unpacked.extend(out / top for top in tops)
    return unpacked


def launcher_version():
    text = (ROOT / "launcher/Cargo.toml").read_text()
    return re.search(r'^version\s*=\s*"([^"]+)"', text, re.M).group(1)


def friends_dll(cfg):
    """build.ini's friends library, else the one in upstream's launcher release of this tree's
    launcher version, downloaded once and checked against the release's SHA256SUMS."""
    if cfg["friends_dll"]:
        if not cfg["friends_dll"].is_file():
            raise Failed(f"build.ini's friends_dll {cfg['friends_dll']} is not a file")
        return cfg["friends_dll"]
    version = launcher_version()
    kept = ROOT / FRIENDS_CACHE / version / FRIENDS_FILE
    if kept.is_file():
        return kept
    zip_name = f"launcher-{version}-windows-x86_64.zip"
    base = f"{LAUNCHER_RELEASES}/launcher-v{version}"
    print(f"Fetching the friends library from {base}/{zip_name}")
    try:
        sums = urllib.request.urlopen(f"{base}/SHA256SUMS").read().decode()
        data = urllib.request.urlopen(f"{base}/{zip_name}").read()
    except OSError as e:
        raise Failed(f"could not fetch the launcher {version} release: {e}") from None
    want = next((l.split()[0] for l in sums.splitlines() if l.endswith(zip_name)), None)
    if want != hashlib.sha256(data).hexdigest():
        raise Failed(f"{zip_name} does not match the release's SHA256SUMS")
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        member = f"launcher-{version}/{FRIENDS_FILE}"
        kept.parent.mkdir(parents=True, exist_ok=True)
        kept.write_bytes(z.read(member))
    return kept


def package(product, stage, cfg, env, allow_dirty, extra=()):
    """Run the project's packager for `product`, writing into a new `stage`."""
    shutil.rmtree(stage, ignore_errors=True)
    command = ["cargo", "xtask", "package", product, "--out", str(stage), *extra]
    if allow_dirty:
        command.append("--allow-dirty")
    status = subprocess.run(command, cwd=ROOT, env=env).returncode
    if status != 0:
        raise Failed(f"the {product} package build failed (exit {status})")


def build_dereth(out, cfg, env, allow_dirty):
    # Dereth's package wraps a launcher release, so the launcher is packaged first.
    launcher = ROOT / STAGE / "launcher"
    mvk = ["--moltenvk", str(cfg["moltenvk_dir"])]
    package("launcher", launcher, cfg, env, allow_dirty, [*mvk, "--friends-dll", str(friends_dll(cfg))])
    stage = ROOT / STAGE / "dereth"
    package("dereth", stage, cfg, env, allow_dirty, [*mvk, "--launcher", str(launcher)])
    apps = [p for p in unpack_outputs(stage, out) if p.suffix == ".app"]
    if not apps:
        raise Failed(f"no .app among the archives in {stage}")
    app = apps[0]
    print(f"\nBuilt: {app}")
    verify(app, cfg, env)
    if cfg["applications_dir"]:
        install(app, cfg["applications_dir"])


def build_empyrean(out, cfg, env, allow_dirty):
    stage = ROOT / STAGE / "empyrean"
    package("empyrean", stage, cfg, env, allow_dirty)
    folders = [p for p in unpack_outputs(stage, out) if p.name.startswith("empyrean-")]
    if not folders:
        raise Failed(f"no empyrean folder among the archives in {stage}")
    for name in EMPYREAN_PROGRAMS:
        program = folders[0] / name
        signed = subprocess.run(
            ["codesign", "--force", "--options", "runtime", "--sign", cfg["identity"], str(program)],
            env=env,
        ).returncode
        if signed != 0:
            raise Failed(f"could not sign {name}")
        verify(program, cfg, env)
    print(f"\nBuilt: {folders[0]}")


def verify(app, cfg, env):
    """Check the signature of `app` as macOS will."""
    if run(["codesign", "--verify", "--deep", "--strict", str(app)], env) is None:
        raise Failed(f"{app.name} does not verify: codesign --verify --deep --strict -v {app}")
    details = subprocess.run(
        ["codesign", "-dv", "--verbose=2", str(app)], text=True, capture_output=True, env=env
    ).stderr
    shown = [l for l in details.splitlines() if l.startswith(("Authority=", "TeamIdentifier=", "CodeDirectory"))]
    print("\n".join("  " + l for l in shown[:4]))
    if "flags=0x10000(runtime)" not in details:
        raise Failed(f"{app.name} is not signed with the hardened runtime, so not with {cfg['identity']}")


def install(app, folder):
    dest = Path(os.path.expanduser(folder)) / app.name
    if dest.exists():
        shutil.rmtree(dest)
    shutil.copytree(app, dest, symlinks=True)
    print(f"Installed {dest}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="check only, build nothing")
    parser.add_argument("--allow-dirty", action="store_true", help="build uncommitted changes too")
    only = parser.add_mutually_exclusive_group()
    only.add_argument("--empyrean-only", action="store_true", help="build the server alone")
    only.add_argument("--dereth-only", action="store_true", help="build the app alone")
    args = parser.parse_args()

    cfg = load_config()
    env = build_env(cfg)

    problems = check(cfg, env, args.allow_dirty)
    if problems:
        raise Failed("cannot build yet:\n  " + "\n  ".join(problems))
    commit = run(["git", "rev-parse", "--short=8", "HEAD"], env).strip()
    out = cfg["out_dir"] / commit
    keep_out_of_dropbox([ROOT / d for d in BUILD_DIRS] + [cfg["out_dir"]])
    print(f"Ready: commit {commit}, signing with {cfg['identity']}, outputs to {out}")
    if args.check:
        return

    # The packager only writes archives, and refuses a folder holding anything else, so each
    # product has its own stage.
    if not args.empyrean_only:
        build_dereth(out, cfg, env, args.allow_dirty)
    if not args.dereth_only:
        build_empyrean(out, cfg, env, args.allow_dirty)
    shutil.rmtree(ROOT / STAGE, ignore_errors=True)
    print("Done.")


if __name__ == "__main__":
    try:
        main()
    except Failed as e:
        sys.exit(f"build.py: {e}")
    except KeyboardInterrupt:
        sys.exit(130)
