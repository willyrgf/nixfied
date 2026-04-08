# Proof Workspace Seed

This seed is a real checked-in workspace fixture.

Scenario bootstrap uses it in two modes:

1. `seed-copy`: copy this workspace, rewrite the local `nixfied` flake input to the source repository under test, and initialize a baseline git commit.
2. `install`: generate a thin or vendored wrapper with `framework::install`, overlay this workspace's proof-owned project files and docs, and initialize a baseline git commit.

The fixture content in `nixfied/project/` is the canonical proof workspace. Bootstrap no longer pulls live `nixfied/project`, `nixfied/modules`, or `nixfied/framework` trees from the source repository.
