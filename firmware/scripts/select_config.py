"""Select only this project's private settings, never an SDK config.h."""
from pathlib import Path

Import("env")  # Provided by PlatformIO/SCons.

if (Path(env.subst("$PROJECT_DIR")) / "include" / "config.h").is_file():
    env.Append(CPPDEFINES=["MRT_PRIVATE_CONFIG"])
