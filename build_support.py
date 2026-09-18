# SPDX-License-Identifier: MIT
"""Include the setup resources without duplicating their source of truth."""
from pathlib import Path
from setuptools.command.build_py import build_py


RUNTIME_FILES = (
    "launch-msfs.sh", "play-msfs.sh", "runtime-env.sh", "xodus.sh",
    "xodus-service.sh", "xodus-wine-launch",
)


class BuildWithResources(build_py):
    def resource_pairs(self):
        root = Path(__file__).resolve().parent
        target = Path(self.build_lib) / "flightdeck" / "resources"
        for name in RUNTIME_FILES:
            yield root / "scripts" / "runtime" / name, target / "runtime" / name
        yield root / "compat" / "upstreams.lock.json", target / "upstreams.lock.json"
        yield root / "compat" / "bootstrap.lock.json", target / "bootstrap.lock.json"

    def run(self):
        super().run()
        for source, target in self.resource_pairs():
            if source.is_symlink() or not source.is_file():
                raise ValueError("Missing regular setup resource: " + source.name)
            self.mkpath(str(target.parent))
            self.copy_file(str(source), str(target))

    def get_outputs(self, include_bytecode=True):
        return super().get_outputs(include_bytecode) + [str(t) for _, t in self.resource_pairs()]
