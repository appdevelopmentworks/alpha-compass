"""Standalone entry point for the packaged sidecar (Nuitka build).

Runs the FastAPI app via uvicorn, reading host / port / token from the
environment (set by the Rust supervisor). This is the entry compiled into the
distributable binary; in dev the sidecar runs via ``uv run`` instead.
"""

from app.main import main

if __name__ == "__main__":
    main()
