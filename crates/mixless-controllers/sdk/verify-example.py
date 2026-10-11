#!/usr/bin/env python3
"""Compile the independently authored C adapter and check its protocol fixture offline."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parents[3]
example = root / "crates/mixless-controllers/examples/synthetic-hid"


def run(*args):
    return subprocess.check_output([str(arg) for arg in args], cwd=root, text=True)


def supports_wasm32(compiler):
    try:
        targets = run(compiler, "--print-targets")
    except (OSError, subprocess.CalledProcessError):
        return False
    return any(line.split()[:1] == ["wasm32"] for line in targets.splitlines())


compiler = os.environ.get("CLANG")
if compiler is None:
    candidates = [shutil.which("clang")]
    brew = shutil.which("brew")
    if brew:
        try:
            candidates.append(Path(run(brew, "--prefix", "llvm").strip()) / "bin/clang")
        except (OSError, subprocess.CalledProcessError):
            pass
    compiler = next((p for p in candidates if p and supports_wasm32(p)), None)
if not compiler or not supports_wasm32(compiler):
    raise SystemExit("Set CLANG to LLVM clang with the wasm32 target (on macOS: brew install llvm)")


def bundled_linker(rustc):
    sysroot = Path(run(rustc, "--print", "sysroot").strip())
    host = next(line.split(": ", 1)[1] for line in run(rustc, "-vV").splitlines()
                if line.startswith("host: "))
    return sysroot / "lib/rustlib" / host / "bin/gcc-ld/wasm-ld"


linker = os.environ.get("WASM_LD")
if linker is None:
    candidates = [shutil.which("wasm-ld"), bundled_linker("rustc")]
    # Homebrew's rustc may precede rustup on PATH and does not bundle wasm-ld.
    # Ask rustup for its selected compiler without changing the user's toolchain.
    rustup = shutil.which("rustup")
    if rustup:
        try:
            rustup_rustc = run(rustup, "which", "rustc").strip()
            candidates.append(bundled_linker(rustup_rustc))
        except (OSError, subprocess.CalledProcessError):
            pass
    linker = next((p for p in candidates if p and Path(p).is_file()), None)
if not linker or not Path(linker).is_file():
    raise SystemExit("Set WASM_LD to wasm-ld (lld or the Rust toolchain's bundled linker)")
run("cargo", "build", "--locked", "-p", "mixless-controllers", "--features", "tools",
    "--bin", "mixless-controller")
tool = root / "target/debug/mixless-controller"
with tempfile.TemporaryDirectory(prefix="mixless-sdk-") as tmp:
    tmp = Path(tmp)
    module, preset, trace = (tmp / n for n in ("adapter.wasm", "preset.json", "trace.json"))
    run(compiler, "--target=wasm32", f"-fuse-ld={linker}", "-O2", "-nostdlib",
        "-Wl,--no-entry", "-Wl,--export-memory", "-Wl,--initial-memory=131072",
        "-Wl,--max-memory=2097152", "-o", module, example / "adapter.c")
    run(tool, "pack", example / "preset.template.json", module, preset)
    run(tool, "validate", preset)
    run(tool, "pin-trace", preset, example / "fixture.json", trace)
    result = json.loads(run(tool, "replay", preset, trace, "--check"))
    assert result["assertions"] == result["frames"] == 8
    assert not any(result["errors"])
    print(f"C SDK example passed: {result['frames']} frames, {result['assertions']} assertions")
