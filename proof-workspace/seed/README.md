# Proof Workspace Seed

This seed is intentionally small and deterministic.

Scenario bootstrap copies this directory to a temporary workspace and then:

1. materializes canonical `nixfied/project` files from the source repository,
2. initializes a baseline git commit,
3. executes scenario assertions through public framework surfaces.
