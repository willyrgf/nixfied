# Navigation only: each topic reads its existing native documentation owner.
{
  commands = {
    file = ../../docs/CONTRACT.md;
    section = "Native command parsing";
  };
  runtime = {
    file = ../../docs/ARCHITECTURE.md;
    section = "Registry, liveness, leases / Ports, state, containment / Output";
  };
  authoring = {
    file = ../../docs/GUIDE.md;
    section = "Author nixfied.nix";
  };
  tasks = {
    file = ../../docs/GUIDE.md;
    section = "Author nixfied.nix / Run and control";
  };
  services = {
    file = ../../docs/ADAPTERS.md;
    section = "What an adapter provides / Conventions";
  };
  state = {
    file = ../../docs/GUIDE.md;
    section = "Services, slots, and state";
  };
  placeholders = {
    file = ../../docs/ADAPTERS.md;
    section = "Endpoints and placeholders";
  };
  secrets = {
    file = ../../docs/GUIDE.md;
    section = "Secrets";
  };
  adapters = {
    file = ../../docs/ADAPTERS.md;
    section = "Parameterization";
  };
  context = {
    file = ../../docs/GUIDE.md;
    section = "Source and invocation context";
  };
  outputs = {
    file = ../../docs/CONTRACT.md;
    section = "Output and failure contract";
  };
  errors = {
    file = ../../docs/CONTRACT.md;
    section = "Runtime error diagnostics";
  };
  recovery = {
    file = ../../docs/GUIDE.md;
    section = "Upgrade and recover";
  };
  discovery = {
    file = ../../docs/GUIDE.md;
    section = "Discover the project surface";
  };
  model = {
    file = ../../docs/CONTRACT.md;
    section = "Model and version boundary";
  };
  derivation = {
    file = ../../docs/DERIVATION_SPEC.md;
    section = "Derived facts";
  };
  development = {
    file = ../../docs/DEVELOPMENT.md;
    section = "Canonical local checks";
  };
}
