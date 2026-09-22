# Independent syntax and boundary vectors; renderHelp is not a static check.
{ lib }:
let
  structure = import ../meta/default.nix { inherit lib; };
  declarations = import ../meta/commands.nix { inherit lib structure; };
  inventory = import ../meta/inventory.nix { inherit lib; } (
    builtins.readFile ../../runtime/crates/nixfied-model/capability.txt
  );
  check =
    declarations:
    import ../meta/syntax.nix { inherit lib; } {
      inherit declarations structure inventory;
      topics = builtins.attrNames (import ../docs/topics.nix);
      publications = import ../project-publications.nix { };
    };
  checked = check declarations;
  rejects = value: !(builtins.tryEval (builtins.deepSeq value true)).success;
  change = f: map (command: if command.name == "run" then f command else command) declarations;
  changeArg =
    f:
    change (
      command:
      command // { arguments = map (arg: if arg.id == "output" then f arg else arg) command.arguments; }
    );
  invalid = f: rejects (check (change f)).commands;
  invalidArg = f: rejects (check (changeArg f)).commands;
  extra = {
    id = "budgetMs";
    binding = "Command";
    token = "--budget-ms";
    valueDomain = {
      kind = "Unsigned";
      bits = 64;
    };
    initialValue = {
      kind = "Literal";
      value = 7;
    };
    help = {
      kind = "Visible";
      text = "Fixture budget";
      metavar = "<number>";
    };
  };
  extended = check (change (command: command // { arguments = command.arguments ++ [ extra ]; }));
  helpThrows = change (command: command // { renderHelp = _: throw "must stay lazy"; });
  helpWrongType = change (command: command // { renderHelp = _: 7; });
  helpPaths = {
    check = ../../runtime/crates/nixfied-runtime/tests/fixtures/help/check.txt;
    run = ../../runtime/crates/nixfied-runtime/tests/fixtures/help/run.txt;
    ps = ../../runtime/crates/nixfied-runtime/tests/fixtures/help/ps.txt;
    down = ../../runtime/crates/nixfied-runtime/tests/fixtures/help/down.txt;
    clean = ../../runtime/crates/nixfied-runtime/tests/fixtures/help/clean.txt;
    install = ../../runtime/crates/nixfied-cli/tests/fixtures/install-help.txt;
    upgrade = ../fixtures/upgrade-help.txt;
  };
in
assert
  map (command: command.name) checked.commands == [
    "check"
    "run"
    "ps"
    "down"
    "clean"
    "install"
    "upgrade"
  ];
assert builtins.all (name: checked.help.${name} + "\n" == builtins.readFile helpPaths.${name}) (
  builtins.attrNames helpPaths
);
assert (check helpThrows).byName.run.name == "run";
assert rejects (check helpThrows).help.run;
assert (check helpWrongType).byName.run.name == "run";
assert rejects (check helpWrongType).help.run;
assert invalid (command: command // { parserPolicy = "last-wins"; });
assert invalid (command: command // { renderHelp = "text"; });
assert invalid (
  command:
  command
  // {
    references = [
      {
        kind = "record";
        id = "missing";
      }
    ];
  }
);
assert invalid (
  command: command // { arguments = command.arguments ++ [ (builtins.head command.arguments) ]; }
);
assert invalid (
  command:
  command // { arguments = command.arguments ++ [ (extra // { id = "outputModeChoices"; }) ]; }
);
# A shared binding cannot change token/domain/default in one command only.
assert rejects
  (check (
    map (
      command:
      if command.name != "down" then
        command
      else
        command
        // {
          arguments = map (
            arg: if arg.id != "slot" then arg else arg // { token = "--other-slot"; }
          ) command.arguments;
        }
    ) declarations
  )).commands;
assert
  builtins.length (
    builtins.filter (arg: arg.symbols.token == "RUNTIME_SLOT") (
      checked.argumentsFor [
        "check"
        "run"
        "ps"
        "down"
        "clean"
      ]
    )
  ) == 1;
assert invalidArg (arg: arg // { token = "--help"; });
assert invalidArg (arg: arg // { id = "task"; });
assert invalidArg (arg: arg // { id = "timeoutMsInitial"; });
assert invalidArg (arg: arg // { token = "--output=x"; });
assert invalidArg (arg: arg // { repetition = "reject"; });
assert invalidArg (
  arg:
  arg
  // {
    valueDomain = {
      kind = "Unknown";
    };
  }
);
assert invalidArg (
  arg:
  arg
  // {
    valueDomain = {
      kind = "Unsigned";
      bits = 16;
    };
  }
);
assert invalidArg (
  arg:
  arg
  // {
    valueDomain = {
      kind = "Enum";
      coordinate = "missing";
    };
  }
);
assert invalidArg (
  arg:
  arg
  // {
    initialValue = {
      kind = "Literal";
      value = "unknown";
    };
  }
);
assert invalidArg (
  arg:
  arg
  // {
    initialValue = {
      kind = "Absent";
      value = null;
    };
  }
);
assert invalidArg (
  arg:
  arg
  // {
    help = {
      kind = "Hidden";
      explanation = "";
    };
  }
);
assert invalidArg (
  arg:
  arg
  // {
    valueDomain = {
      kind = "Flag";
    };
    initialValue = {
      kind = "Literal";
      value = false;
    };
  }
);
assert invalidArg (
  arg:
  arg
  // {
    valueDomain = {
      kind = "Unsigned";
      bits = 32;
    };
    initialValue = {
      kind = "Literal";
      value = 4294967296;
    };
  }
);
assert lib.hasInfix "  --budget-ms <number>    Fixture budget\n  -h, --help" extended.help.run;
assert !(lib.hasInfix "--model" extended.help.run);
assert lib.hasInfix "type RunBudgetMsValue = u64;" (
  (import ../meta/syntax-project.nix { inherit lib structure; } extended).rust [ "run" ]
);
true
