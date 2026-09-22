# Independent graph vectors: identities, checked selection and derived inverses.
{ lib }:
let
  assemble = import ../docs/navigation.nix { inherit lib; };
  ref = kind: id: { inherit kind id; };
  entry = kind: id: references: { inherit kind id references; };
  option = path: (entry "option" (lib.concatStringsSep "." path) [ ]) // { loc = path; };
  topic = select: {
    inherit select;
    related = [ ];
  };
  run = ref "command" "run";
  app = ref "app" "run";
  alpha = ref "topic" "alpha";
  entries = [
    (entry "command" "run" [
      alpha
      alpha
    ])
    (entry "app" "run" [ run ])
    (option [
      "nixfied"
      "state"
      "policy"
    ])
    (option [
      "nixfied"
      "stateful"
      "policy"
    ])
  ];
  topics = {
    alpha = (topic [ run ]) // {
      related = [ "beta" ];
    };
    beta =
      (topic [
        {
          kind = "option-namespace";
          path = [
            "nixfied"
            "state"
          ];
        }
      ])
      // {
        related = [ "alpha" ];
      };
  };
  graph = assemble { inherit entries topics; };
  rejects = value: !(builtins.tryEval (builtins.deepSeq value true)).success;
  rejectsSelector =
    selector:
    rejects (assemble {
      inherit entries;
      topics = {
        alpha = topic [ selector ];
      };
    });
in
assert graph.references run == [ alpha ];
assert graph.references app == [ run ];
assert graph.backlinks run == [ app ];
assert graph.members "alpha" == [ run ];
assert
  (assemble {
    inherit entries;
    topics.alpha = topic [
      app
      run
      app
    ];
  }).members
    "alpha" == [
    app
    run
  ];
assert
  (assemble {
    inherit entries;
    topics.alpha = topic [ app ];
  }).members
    "alpha" == [
    app
    run
  ];
assert
  graph.backlinks alpha == [
    run
    (ref "topic" "beta")
  ];
assert graph.references (ref "option" "nixfied.state.policy") == [ (ref "topic" "beta") ];
assert graph.references (ref "option" "nixfied.stateful.policy") == [ ];
assert
  (assemble {
    inherit entries;
    topics.alpha = topic [ { kind = "command"; } ];
  }).backlinks
    alpha == [ run ];
assert
  (assemble {
    inherit entries;
    topics.alpha = topic [
      {
        kind = "option";
        path = [
          "nixfied"
          "state"
          "policy"
        ];
      }
    ];
  }).references
    (ref "option" "nixfied.state.policy") == [ alpha ];
assert rejects (assemble {
  entries = entries ++ [ (entry "command" "run" [ ]) ];
  inherit topics;
});
assert rejects (assemble {
  entries = entries ++ [ (entry "package" "unqueried" [ (ref "command" "missing") ]) ];
  inherit topics;
});
assert rejects (assemble {
  entries = entries ++ [
    (entry "package" "unqueried" [
      {
        kind = "command";
        id = "run";
        extra = true;
      }
    ])
  ];
  inherit topics;
});
assert rejects (assemble {
  inherit entries;
  topics.alpha = (topic [ ]) // {
    related = [ "missing" ];
  };
});
assert rejectsSelector (ref "record" "run");
assert rejectsSelector (ref "command" "missing");
assert rejectsSelector { kind = "unknown"; };
assert rejectsSelector { kind = "vocabulary"; };
assert rejectsSelector {
  kind = "option";
  id = "nixfied.state.policy";
};
assert rejectsSelector {
  kind = "option";
  path = [
    "nixfied"
    "state"
  ];
};
assert rejectsSelector {
  kind = "option-namespace";
  path = [
    "nixfied"
    "sta"
  ];
};
assert rejectsSelector {
  kind = "option-namespace";
  path = [ ];
};
assert rejectsSelector {
  kind = "option-namespace";
  path = [
    "nixfied"
    1
  ];
};
assert rejectsSelector {
  kind = "command";
  id = "run";
  extra = true;
};
assert rejectsSelector "run";
true
