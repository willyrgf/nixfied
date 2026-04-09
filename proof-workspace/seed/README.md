# Proof Workspace Seed

This seed is a real checked-in workspace fixture.

Scenario bootstrap uses it in two modes:

1. `seed-copy`: copy this minimal workspace, rewrite its framework input to `path:$REPO_ROOT`, and initialize a baseline git commit.
2. `install`: generate a thin or vendored wrapper with `framework::install`, overlay this workspace's proof-owned project files and docs, and initialize a baseline git commit.

The fixture content in `nixfied/project/` and `nixfied/local/` is the canonical proof workspace. The seed does not carry a checked-in framework snapshot; temp seed copies consume the source repository framework through the rewritten `nixfied` flake input, and install/upgrade proofs materialize wrappers only in temporary directories.
