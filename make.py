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
    build_go()


def build_frontend():
    """Build the static frontend files"""
    run("bun i --cwd frontend")
    run("bun run --cwd frontend build")


def build_go():
    """Build the Go backend"""
    target = os.environ.get("TARGET", "bin/gnuplex")
    run(f'go build -C backend -o {target} -ldflags "-X main.SourceHash={_source_hash()} ' + f"-X main.Platform={platform()} " + f"-X main.GoVersion={go_version()} " + '" .')


def build_go_ci():
    """Build the Go backend (used by CI)"""
    run("go build -C backend -o /tmp/gnuplex" + f' -ldflags "-X main.SourceHash={_source_hash()}' + f' -X main.Platform={platform()}" .')


def bump_version():
    """Bump the version of this repo"""
    ver = str(int(time.time()))
    run('sed -E -i -e "s/Version = \\"[0-9]+\\"/Version = \\"' + ver + '\\"/" backend/consts/version.go')


def dev():
    """Run a local development server (with hot frontend reloading)"""
    remove_sockets()
    run("bun i --cwd frontend")
    os.makedirs("tmp", exist_ok=True)
    os.makedirs("tmp/screenshots", exist_ok=True)
    procs = [
        subprocess.Popen("caddy run", shell=True),
        subprocess.Popen("bun run --cwd frontend dev", shell=True),
        subprocess.Popen("go run -C backend . -verbose -static_files ../tmp", shell=True),
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
        subprocess.Popen("./backend/bin/gnuplex -verbose -static_files ./backend/static -config_dir ./backend/mpv_config", shell=True),
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
    run("go -C backend fix ./...")
    run("find backend -name '*.go' -print0 | xargs -0 gofmt -w -s")
    run("bun i")
    run("bun run oxfmt")
    run("bun run oxlint --fix-dangerously")
    run("uv run ruff format")
    run("uv run ruff check --fix")
    run("cd backend && go mod tidy")


def go_source_hash():
    """Prints a unique hash for the repo's current source code"""
    print(_source_hash())


def go_version():
    return ".".join(subprocess.check_output("go version", shell=True).decode().strip().split(" ")[2:]).replace("/", "-")


def lint():
    """Lint this repo"""
    git_status = git_status_clean()
    golint_out = run_stdout("gofmt -s -d backend", should_log=True)
    if len(golint_out) > 0:
        log.error("go lint failed. run ./make.py fmt to fix.")
        sys.exit(1)
    run("bun i")
    run("bun run oxfmt --check")
    run("bun run oxlint")
    run("uv run ruff format --check")
    run("uv run ruff check")
    # run go mod tidy and make sure git status is clean after
    run("cd backend && go mod tidy")
    if not git_status_clean():
        if git_status:
            log.error("git status is not clean after go mod tidy.")
        else:
            log.error("git status is not clean.")
        sys.exit(1)


def test():
    """Run Go tests"""
    run("go test -C backend ./...")


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


def set_go_version():
    """Set the Go version across all configuration files"""
    if len(sys.argv) < 3:
        print("Usage: make.py set_go_version <version>")
        print("Example: make.py set_go_version 1.26.0")
        sys.exit(1)

    version = sys.argv[2]

    # Update .github/workflows/release-linux-glibc-x86_64.yml
    run(f'sed -E -i -e "s/go-version: \\"\\^[0-9]+\\.[0-9]+\\.[0-9]+\\"/go-version: \\"^{version}\\"/" .github/workflows/release-linux-glibc-x86_64.yml')

    # Update .github/workflows/test-branch.yml
    run(f'sed -E -i -e "s/go-version: \\"\\^[0-9]+\\.[0-9]+\\.[0-9]+\\"/go-version: \\"^{version}\\"/" .github/workflows/test-branch.yml')

    # Update backend/go.mod
    run(f'sed -E -i -e "s/^go [0-9]+\\.[0-9]+\\.[0-9]+$/go {version}/" backend/go.mod')


def git_status_clean():
    """Check if the git status is clean"""
    repo = pygit2.Repository(Path(__file__).parent)
    status = repo.status()
    return len(status) == 0


def _source_hash():
    backend = Path(__file__).parent / "backend"
    files = sorted(backend.rglob("*.go"))
    sha = hashlib.sha256()
    for f in files:
        sha.update(str(f).encode())
        with open(f, "rb") as fh:
            sha.update(fh.read())
    return sha.hexdigest()


TASKS = {
    "build_go_ci": build_go_ci,
    "build_go": build_go,
    "build": build,
    "bump_version": bump_version,
    "dev_compiled": dev_compiled,
    "dev": dev,
    "fmt": fmt,
    "build_frontend": build_frontend,
    "go_source_hash": go_source_hash,
    "lint": lint,
    "set_go_version": set_go_version,
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
