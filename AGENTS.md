# GNUPlex codebase

## About

See README.md.

The backend is written in Go, in `./backend`.

The frontend is written in Vite/React/Tailwind, in `./frontend`.

`make.py` lists all common dev tasks, like building frontend/backend and formatting/linting.

## To Remember

Always use `bun` rather than `node` or `npm` in `./frontend`.

Always run `uv run make.py`, not `python make.py`

Make you're not on the git main branch, and commit progress as you work!

## Code checks:

After writing code, confirm code builds successfully by running: `uv run make.py build`.

You can build just the backend by running `uv run make.py go_build`. You can build just the frontend by running `uv run make.py frontend_build`.

Confirm that the code meets best practices by running: `uv run make.py lint`.
