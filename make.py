#!/usr/bin/env -S uv run --script
import glob
import hashlib
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

import pygit2
import structlog

log = structlog.get_logger()
from rich.traceback import install

install(show_locals=True)


def build():
    """Build the frontend and the backend"""
    build_frontend()
    build_backend()


def build_frontend():
    """Build the static frontend files"""
    run("bun i --cwd frontend")
    run("bun run --cwd frontend build")


def build_backend(target="backend/bin/gnuplex"):
    """Build the Rust backend"""
    run("cargo build --release --manifest-path backend/Cargo.toml", env=build_env())
    os.makedirs(os.path.dirname(target), exist_ok=True)
    run(f"cp backend/target/release/gnuplex {target}")


def build_ci():
    """Build the Rust backend (used by CI)"""
    build_backend("/tmp/gnuplex")


def bump_version():
    """Bump the version of this repo"""
    ver = str(int(time.time()))
    run('sed -E -i -e "s/(pub const VERSION: &str = \\")[0-9]+(\\";)/\\1' + ver + '\\2/" backend/src/consts.rs')


def dev():
    """Run a local development server (with hot frontend reloading)"""
    remove_sockets()
    run("bun i --cwd frontend")
    os.makedirs("tmp", exist_ok=True)
    os.makedirs("tmp/screenshots", exist_ok=True)
    procs = [
        subprocess.Popen("caddy run", shell=True),
        subprocess.Popen("bun run --cwd frontend dev", shell=True),
        subprocess.Popen(
            "cargo run --manifest-path backend/Cargo.toml -- --verbose --static-files ./tmp --config-dir ./backend/mpv_config",
            shell=True,
        ),
    ]

    def cleanup(signum, frame):
        for p in procs:
            p.terminate()
        sys.exit(0)

    signal.signal(signal.SIGTERM, cleanup)
    try:
        for p in procs:
            p.wait()
    except KeyboardInterrupt:
        cleanup(None, None)


def dev_compiled():
    """Run a local development server against a compiled frontend/backend"""
    build()
    remove_sockets()
    os.makedirs("tmp", exist_ok=True)
    procs = [
        subprocess.Popen("caddy run --config Caddyfile-compiled", shell=True),
        subprocess.Popen(
            "./backend/bin/gnuplex --verbose --static-files ./backend/static --config-dir ./backend/mpv_config",
            shell=True,
        ),
    ]

    def cleanup(signum, frame):
        for p in procs:
            p.terminate()
        sys.exit(0)

    signal.signal(signal.SIGTERM, cleanup)
    try:
        for p in procs:
            p.wait()
    except KeyboardInterrupt:
        cleanup(None, None)


def fmt():
    """Format this repo"""
    run("cargo fmt --manifest-path backend/Cargo.toml")
    run("bun i")
    run("bun run oxfmt")
    run("bun run oxlint --fix-dangerously")
    run("uv run ruff format")
    run("uv run ruff check --fix")


def source_hash():
    """Prints a unique hash for the repo's current source code"""
    print(_source_hash())


def rust_version():
    return "rustc " + run_stdout("rustc --version").split()[1]


def lint():
    """Lint this repo"""
    git_status = git_status_clean()
    rustfmt_out = run_stdout("cargo fmt --manifest-path backend/Cargo.toml -- --check", should_log=True)
    if len(rustfmt_out) > 0:
        log.error("rust lint failed. run ./make.py fmt to fix.")
        sys.exit(1)
    run("cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings")
    run("bun i")
    run("bun run oxfmt --check")
    run("bun run oxlint")
    run("uv run ruff format --check")
    run("uv run ruff check")
    if not git_status_clean():
        if git_status:
            log.error("git status is not clean")
        else:
            log.error("git status is not clean.")
        sys.exit(1)


def test():
    """Run Rust tests"""
    run("cargo test --manifest-path backend/Cargo.toml")


def platform():
    os = run_stdout("uname -s")
    arch = run_stdout("uname -m")
    libc = "musl" if "musl" in run_stdout("ldd /bin/ls") else "glibc"
    return f"{os}-{libc}-{arch}".lower()


def remove_sockets():
    """Clean up stale mpv socket files from /tmp"""
    for socket in glob.glob("/tmp/mpvsocket-*"):
        try:
            os.remove(socket)
        except OSError as e:
            print(f"Failed to remove {socket}: {e}")


def run(cmd, **kwargs):
    log.info(f"+ {cmd}")
    res = subprocess.run(cmd, shell=True, check=True, **kwargs)
    if res.returncode != 0:
        log.error("cmd failed. exiting...")
        sys.exit(res.returncode)


def run_stdout(cmd, error_status_ok=True, should_log=False):
    if should_log:
        log.info(f"+ {cmd}")
    try:
        return subprocess.check_output(cmd, shell=True).decode().strip()
    except subprocess.CalledProcessError as e:
        if error_status_ok:
            return e.output.decode().strip()
        log.error(f"cmd failed: {e.output.decode().strip()}")
        sys.exit(e.returncode)


def set_rust_version():
    """Set the minimum Rust version across all configuration files"""
    if len(sys.argv) < 3:
        print("Usage: make.py set_rust_version <version>")
        print("Example: make.py set_rust_version 1.100.0")
        sys.exit(1)

    version = sys.argv[2]
    run(f'sed -E -i -e "s/^rust-version = \\"[0-9]+\\.[0-9]+\\.[0-9]+\\"$/rust-version = \\"{version}\\"/" backend/Cargo.toml')


def git_status_clean():
    """Check if the git status is clean"""
    repo = pygit2.Repository(Path(__file__).parent)
    status = repo.status()
    return len(status) == 0


def _source_hash():
    backend = Path(__file__).parent / "backend" / "src"
    files = sorted(backend.rglob("*.rs"))
    sha = hashlib.sha256()
    for f in files:
        sha.update(str(f.relative_to(backend)).encode())
        with open(f, "rb") as fh:
            sha.update(fh.read())
    return sha.hexdigest()


def build_env():
    return {
        **os.environ,
        "GNUPLEX_SOURCE_HASH": _source_hash(),
        "GNUPLEX_PLATFORM": platform(),
        "GNUPLEX_COMPILER_VERSION": rust_version(),
    }


TASKS = {
    "build_ci": build_ci,
    "build_backend": build_backend,
    "build": build,
    "bump_version": bump_version,
    "dev_compiled": dev_compiled,
    "dev": dev,
    "fmt": fmt,
    "build_frontend": build_frontend,
    "source_hash": source_hash,
    "lint": lint,
    "set_rust_version": set_rust_version,
    "test": test,
}


def main():
    if len(sys.argv) < 2:
        print("Available tasks:")
        for t in sorted(TASKS):
            print(f"\t{t:25s}\t{TASKS[t].__doc__ or ''}")
        sys.exit(1)
    task = sys.argv[1].lower()
    if task not in TASKS:
        print(f"Unknown task: {task}")
        sys.exit(1)
    TASKS[task]()


if __name__ == "__main__":
    main()
