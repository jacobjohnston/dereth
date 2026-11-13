#!/usr/bin/env python3
"""Run the local Empyrean world from what build.py built, and log in to it.

    python3 local.py            start the world, then the client, logged in
    python3 local.py --server   start the world alone
    python3 local.py --client   start the client alone, for a world already running
    python3 local.py --launcher open the launcher, to play on other people's worlds

The first run makes the world's folder: its world database, downloaded and built once, its
empyrean.toml and a password for its account. The server's console stays in this terminal: `exit`
or one Ctrl-C saves and stops the world.

The client plays with what the launcher chose for this world (the custom world at 127.0.0.1 or
localhost on this port): its data files, its classic set and its art pack, or else the Library's.
Without such a world in the launcher, it plays with build.ini's data files and no art pack.

Settings live under [local] in build.ini.
"""

import argparse
import configparser
import json
import secrets
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CONFIG = ROOT / "build.ini"
# The launcher's choices: its Library and what each world plays with.
LAUNCHER_STATE = Path.home() / "Library/Application Support/Dereth/launcher/launcher-state.json"

# Builds are taken from this branch only; upstream builds lack its custom server rules.
BRANCH = "local/combined"

ACCOUNT_FILE = "local-account.ini"
# How long the world may take to load before the client is started anyway.
READY_SECONDS = 180


class Failed(Exception):
    pass


def resolve(path):
    p = Path(path.strip()).expanduser()
    return p if p.is_absolute() else ROOT / p


def load_config():
    cfg = configparser.ConfigParser()
    cfg.read(CONFIG)
    try:
        return {
            "out_dir": resolve(cfg["build"]["out_dir"]),
            "data_dir": resolve(cfg["build"]["data_dir"]),
            "world_dir": resolve(cfg["local"]["world_dir"]),
            "world_name": cfg["local"]["world_name"].strip(),
            "port": cfg["local"].getint("port"),
            "account": cfg["local"]["account"].strip(),
        }
    except KeyError as e:
        raise Failed(f"{CONFIG.name} is missing {e}") from None


def on_branch(commit):
    """Whether `commit` (a build folder's name) is on BRANCH."""
    return subprocess.run(
        ["git", "merge-base", "--is-ancestor", commit, BRANCH],
        cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    ).returncode == 0


def newest(out_dir, pattern, what):
    """The newest build of BRANCH matching `pattern` under `out_dir`, by when it was written."""
    found = [p for p in out_dir.glob(pattern)
             if p.exists() and on_branch(p.relative_to(out_dir).parts[0])]
    if not found:
        raise Failed(f"no {what} built from {BRANCH} in {out_dir}: run python3 build.py --allow-dirty")
    return max(found, key=lambda p: p.stat().st_mtime)


def toml_string(value):
    return '"' + str(value).replace("\\", "\\\\").replace('"', '\\"') + '"'


def prepare_world(cfg, importer):
    """Make what the world's folder lacks; what is already there is kept."""
    world = cfg["world_dir"]
    world.mkdir(parents=True, exist_ok=True)
    if not (cfg["data_dir"] / "client_portal.dat").is_file():
        raise Failed(f"no game data files in {cfg['data_dir']}")
    if not (world / "world.pack").is_file():
        print("Building the world database (a download of about 150 MB, once)...")
        if subprocess.run([str(importer), "fetch", "--pack"], cwd=world).returncode != 0:
            raise Failed("could not build the world database")
    config = world / "empyrean.toml"
    if not config.is_file():
        config.write_text(
            "[server]\n"
            f"world_name = {toml_string(cfg['world_name'])}\n"
            f"dat_files_directory = {toml_string(cfg['data_dir'])}\n"
            'world_pack_path = "./world.pack"\n'
            'log_level = "info"\n'
            "\n"
            "[server.network]\n"
            "# Only this computer can connect. See empyrean.toml.example for every key.\n"
            'host = "127.0.0.1"\n'
            f"port = {cfg['port']}\n"
        )
        print(f"Wrote {config}")


def account_password(cfg):
    """The account's password, made the first time and kept readable by this user alone."""
    path = cfg["world_dir"] / ACCOUNT_FILE
    stored = configparser.ConfigParser()
    stored.read(path)
    if stored.has_option("account", "password"):
        return stored["account"]["password"]
    password = secrets.token_urlsafe(18)
    stored["account"] = {"password": password}
    path.touch(mode=0o600)
    with path.open("w") as f:
        stored.write(f)
    return password


def status_address(cfg):
    """Where the world's status endpoint is: on the command line, so any empyrean.toml has it."""
    return f"127.0.0.1:{cfg['port'] + 100}"


def world_is_up(cfg):
    url = f"http://{status_address(cfg)}/health"
    try:
        with urllib.request.urlopen(url, timeout=1) as answer:
            return answer.status == 200
    except (urllib.error.URLError, OSError, ValueError):
        return False


def launcher_choices(cfg):
    """The data files, classic set and art pack the launcher chose for this world, as the launcher
    chooses them when it plays it: the world's last choices that still stand, else the Library's
    first modern set and its default pack. `None` for each when the launcher knows no such world."""
    try:
        state = json.loads(LAUNCHER_STATE.read_text())
    except (OSError, ValueError):
        return None, None, None
    slug = next(
        (w["slug"] for w in state.get("custom_worlds", [])
         if w.get("host") in ("127.0.0.1", "localhost") and w.get("port") == cfg["port"]),
        None,
    )
    if slug is None:
        return None, None, None
    prefs = state.get("world_prefs", {}).get(slug, {})
    sets = {s["id"]: s for s in state.get("dat_sets", [])}
    chosen = sets.get(prefs.get("dat_set_id"))
    if chosen is None or chosen.get("kind") != "modern":
        chosen = next((s for s in state.get("dat_sets", []) if s.get("kind") == "modern"), None)
    classic = sets.get(prefs.get("classic_set_id"))
    packs = state.get("art_packs", [])
    wanted = prefs.get("art_pack_id")
    default = next((a for a in packs if a.get("is_default")), None)
    pack = None if wanted == "" else next((a for a in packs if a["id"] == wanted), default)
    if pack is not None and not Path(pack["path"]).exists():
        print(f"The art pack at {pack['path']} is not there: playing without one.")
        pack = None
    return (
        Path(chosen["path"]) if chosen else None,
        Path(classic["path"]) if classic and classic.get("kind") == "classic" else None,
        Path(pack["path"]) if pack else None,
    )


def start_launcher(cfg):
    """Open the newest build's launcher, the way to play on other people's worlds."""
    app = newest(cfg["out_dir"], "*/Dereth.app", "Dereth.app")
    if subprocess.run(["open", str(app)]).returncode != 0:
        raise Failed(f"could not open {app}")
    print(f"Opened the launcher in {app}")


def start_client(cfg):
    """Start the client on its own, so a Ctrl-C at the server's console leaves it to close."""
    app = newest(cfg["out_dir"], "*/Dereth.app", "Dereth.app")
    dats, classic, pack = launcher_choices(cfg)
    dats = dats or cfg["data_dir"]
    extra = (["--classic-dat-dir", str(classic)] if classic else []) + (
        ["--art-pack", str(pack)] if pack else [])
    print(f"The client plays with the data files in {dats}"
          + (f" and the art pack {pack}" if pack else ", and no art pack") + ".")
    log = (cfg["world_dir"] / "client.log").open("w")
    subprocess.Popen(
        [
            str(app / "Contents/MacOS/dereth-client"),
            "-a", cfg["account"],
            "-v", account_password(cfg),
            "-h", "127.0.0.1",
            "-p", str(cfg["port"]),
            "--dat-dir", str(dats),
            *extra,
        ],
        cwd=cfg["world_dir"],
        stdin=subprocess.DEVNULL,
        stdout=log,
        stderr=log,
        start_new_session=True,
    )
    print(f"\nStarted the client from {app}, logging in as {cfg['account']}.")


def client_when_ready(cfg, server):
    """Start the client once the world answers, unless the server has stopped."""
    deadline = time.monotonic() + READY_SECONDS
    while time.monotonic() < deadline and server.poll() is None:
        if world_is_up(cfg):
            start_client(cfg)
            return
        time.sleep(0.5)
    if server.poll() is None:
        print("\nThe world has not answered yet; start the client with: python3 local.py --client")


def run_server(cfg, with_client):
    folder = newest(cfg["out_dir"], "*/empyrean-*/empyrean-server", "Empyrean").parent
    prepare_world(cfg, folder / "empyrean-import")
    server = subprocess.Popen(
        [str(folder / "empyrean-server"), "--status", status_address(cfg)], cwd=cfg["world_dir"]
    )
    if with_client:
        threading.Thread(target=client_when_ready, args=(cfg, server), daemon=True).start()
    # A Ctrl-C reaches the server too, which is saving: wait for it rather than leave it behind.
    while True:
        try:
            return server.wait()
        except KeyboardInterrupt:
            pass


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--server", action="store_true", help="start the world alone")
    mode.add_argument("--client", action="store_true", help="start the client alone")
    mode.add_argument(
        "--launcher", action="store_true", help="open the launcher, to play on other worlds"
    )
    args = parser.parse_args()

    cfg = load_config()
    if args.launcher:
        start_launcher(cfg)
        return 0
    if args.client or (not args.server and world_is_up(cfg)):
        if not world_is_up(cfg):
            raise Failed("the world is not running: start it with python3 local.py --server")
        start_client(cfg)
        return 0
    if world_is_up(cfg):
        raise Failed("the world is already running")
    return run_server(cfg, not args.server)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Failed as e:
        sys.exit(f"local.py: {e}")
