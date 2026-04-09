# Proof Workspace Seed Guidelines

- Keep this seed small and deterministic.
- Do not commit local machine-specific paths or secrets.
- Keep proof behavior owned by checked-in fixture content under `nixfied/project/` and `nixfied/local/`.
- Keep `proof-workspace/seed/nixfied/` limited to `project/` and `local/`; framework-owned trees belong only in temporary install/upgrade materializations.
