# Proof Workspace Seed Guidelines

- Keep this seed small and deterministic.
- Do not commit local machine-specific paths or secrets.
- Keep proof behavior owned by checked-in fixture content under `nixfied/project/`.
- Keep the checked-in vendored `nixfied/` snapshot aligned with the current framework revision when seed runtime behavior changes.
