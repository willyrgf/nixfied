# Independent policy vectors; the expected bytes are not rendered declarations.
{ lib }:
let
  declarations = import ../meta/model.nix { inherit lib; };
  inventory = import ../meta/inventory.nix { inherit lib; } (
    builtins.readFile ../../runtime/crates/nixfied-model/capability.txt
  );
  check = bundle: import ../meta/structure.nix { inherit lib; } (bundle // { inherit inventory; });
  checked = check declarations;
  rejects = value: !(builtins.tryEval (builtins.deepSeq value true)).success;
  changeRecord =
    name: f:
    declarations
    // {
      records = map (record: if record.rust.name == name then f record else record) declarations.records;
    };
  changeField =
    recordName: fieldName: f:
    changeRecord recordName (
      record:
      record
      // {
        fields = map (field: if field.name == fieldName then f field else field) record.fields;
      }
    );
  project =
    bundle:
    (check bundle).constructors."primitive/Project" {
      projectId = "id";
      name = "Name";
    };
  invalidField = f: rejects (project (changeField "TaskSpec" "servicesRequired" f));
  make = name: checked.constructors.${"primitive/${name}"};
  invocation = {
    tools = [ "tool" ];
    run = [ "tool" ];
    executable = "/nix/store/tool/bin/tool";
    env = { };
    codebaseId = "main";
    cwd = ".";
    stdin = "null";
    timeoutMs = 1;
  };
  task = {
    kind = "composite";
    defaultOutput = "summary";
    serviceLifetime = "run-scoped";
    servicesRequired = [ ];
  };
  field = {
    name = "value";
    description = "Synthetic independent nullable field.";
    value = {
      kind = "Text";
    };
    decode = {
      kind = "Nullable";
    };
    rustEncode = "Present";
    nixEncode = "RequiredPresent";
    rust = {
      visibility = "pub";
      storage = "Direct";
    };
  };
  synthetic = {
    identity = {
      kind = "Local";
      name = "NullableFixture";
    };
    description = "Required member accepting explicit null.";
    decoder = "RejectUnknown";
    fields = [ field ];
    rust = {
      file = "fixture.rs";
      module = [ "fixture" ];
      name = "NullableFixture";
      visibility = "pub";
      emission = "Owned";
      derives = [
        "Debug"
        "PartialEq"
      ];
    };
  };
  fixture = check {
    records = [ synthetic ];
    vocabularies = [ ];
  };
  childRef = {
    kind = "RecordRef";
    identity = synthetic.identity;
  };
  listOf = unique: element: {
    kind = "List";
    inherit unique element;
  };
  native = {
    kind = "NativeDomain";
    id = "CheckedText";
    description = "Native construction cannot be certified from its wire type.";
    wire = {
      kind = "Text";
    };
    rustPath = [ "String" ];
    producerCheck = _: throw "default validation invoked native code";
  };
  nested =
    child: value: decode:
    check {
      vocabularies = [ ];
      records = [
        child
        (
          synthetic
          // {
            identity = {
              kind = "Local";
              name = "ParentFixture";
            };
            rust = synthetic.rust // {
              name = "ParentFixture";
            };
            fields = [ (field // { inherit value decode; }) ];
          }
        )
      ];
    };
  wire =
    child: input:
    (nested child childRef { kind = "Required"; }).constructors."local/ParentFixture" {
      value = input;
    };
  literal =
    child: value: input:
    map (record: record.id)
      (nested child value {
        kind = "Default";
        literal = input;
      }).records;
  withField = f: synthetic // { fields = [ (f field) ]; };
  optionalChild = withField (
    f:
    f
    // {
      decode.kind = "Optional";
      nixEncode = "RequiredOmitAbsent";
    }
  );
  emptyChild = withField (
    f:
    f
    // {
      value = listOf false { kind = "Text"; };
      decode = {
        kind = "Default";
        literal = [ ];
      };
      nixEncode = "RequiredOmitEmpty";
    }
  );
  nativeChild = optionalChild // {
    fields = map (f: f // { value = native; }) optionalChild.fields;
  };
  outputs = import ../meta/outputs.nix { inherit lib; };
  outputCheck =
    bundle:
    import ../meta/structure.nix { inherit lib; } (
      bundle
      // {
        inherit inventory;
        recoveryTopics = builtins.attrNames (import ../docs/topics.nix);
      }
    );
  outputTypes = outputCheck outputs;
  errorMutation =
    f:
    outputs
    // {
      vocabularies = map (
        vocabulary: if vocabulary.coordinate == "error-code" then f vocabulary else vocabulary
      ) outputs.vocabularies;
    };
  outputMutation =
    name: f:
    outputs
    // {
      records = map (record: if record.rust.name == name then f record else record) outputs.records;
    };
  rejectsOutput = bundle: rejects (map (record: record.id) (outputCheck bundle).records);
in
assert wire synthetic { value = null; } == { value.value = null; };
assert rejects (wire synthetic { });
assert wire optionalChild { } == { value = { }; };
assert rejects (wire optionalChild { value = null; });
assert wire emptyChild { } == { value = { }; };
assert rejects (wire emptyChild { value = [ ]; });
assert rejects (
  wire (withField (
    f:
    f
    // {
      decode = {
        kind = "Default";
        literal = "default";
      };
    }
  )) { }
);
assert
  wire (withField (
    f:
    f
    // {
      decode.kind = "Optional";
      nixEncode = "PreserveSupplied";
    }
  )) { value = null; } == {
    value.value = null;
  };
assert
  (nested optionalChild childRef { kind = "Required"; }).constructors."local/NullableFixture" {
    value = null;
  } == { };
assert
  (nested emptyChild childRef { kind = "Required"; }).constructors."local/NullableFixture" {
    value = [ ];
  } == { };
assert rejects (literal synthetic native "text");
assert rejects (literal synthetic (listOf false native) [ "text" ]);
assert literal synthetic (listOf false native) [ ] != [ ];
assert
  literal synthetic {
    kind = "Map";
    value = native;
  } { } != [ ];
assert literal nativeChild childRef { } != [ ];
assert literal nativeChild childRef { value = null; } != [ ];
assert rejects (literal nativeChild childRef { value = "text"; });
assert rejects (
  literal optionalChild (listOf true childRef) [
    { }
    { value = null; }
  ]
);
assert rejects (
  literal optionalChild (listOf true {
    kind = "Map";
    value = childRef;
  }) [ { child = { }; } ]
);
assert literal optionalChild (listOf true childRef) [ ] != [ ];
assert
  literal optionalChild (listOf false childRef) [
    { }
    { value = null; }
  ] != [ ];
assert
  literal synthetic (listOf true { kind = "Text"; }) [
    "one"
    "two"
  ] != [ ];
assert rejects (
  literal synthetic (listOf true { kind = "Text"; }) [
    "one"
    "one"
  ]
);
assert rejects (
  literal synthetic childRef {
    value = null;
    extra = true;
  }
);
assert
  literal (synthetic // { decoder = "IgnoreUnknown"; }) childRef {
    value = null;
    extra = true;
  } != [ ];
assert rejects (
  literal (synthetic // { decoder = "IgnoreUnknown"; }) childRef {
    value = null;
    ignored = x: x;
  }
);
assert rejects (literal (synthetic // { decoder = "NoDecoder"; }) childRef { value = null; });
assert builtins.length outputTypes.records == 19;
assert
  builtins.length (builtins.filter (record: record.rust.emission == "Owned") outputTypes.records)
  == 13;
assert
  builtins.length (builtins.filter (record: record.rust.emission == "Borrowed") outputTypes.records)
  == 5;
assert
  builtins.length (
    builtins.filter (record: record.rust.emission == "MemberNamesOnly") outputTypes.records
  ) == 1;
assert outputTypes.constructors == { };
assert rejectsOutput (
  errorMutation (v: v // { annotations = builtins.removeAttrs v.annotations [ "TASK_FAILED" ]; })
);
assert rejectsOutput (
  errorMutation (
    v:
    v
    // {
      annotations = v.annotations // {
        TASK_FAILED = {
          description = "Known error.";
          recoveryTopic = "missing";
        };
      };
    }
  )
);
assert rejectsOutput (
  outputMutation "ProjectionDiagnostic" (r: r // { decoder = "IgnoreUnknown"; })
);
assert rejectsOutput (
  outputMutation "PortConflictDetails" (
    r:
    r
    // {
      fields = map (
        f:
        f
        // {
          rust = f.rust // {
            storage = "Box";
          };
        }
      ) r.fields;
    }
  )
);
assert rejectsOutput (
  outputMutation "RuntimeError" (
    r:
    r
    // {
      fields = map (
        f:
        if f.name == "message" then
          f
          // {
            value = {
              kind = "RecordRef";
              identity = {
                kind = "Inventory";
                coordinate = "output-schema runtime-error-projection";
              };
            };
          }
        else
          f
      ) r.fields;
    }
  )
);
assert rejectsOutput (
  outputMutation "PortConflictDetails" (
    r:
    r
    // {
      fields = map (
        f:
        if f.name == "endpoint" then
          f
          // {
            value = {
              kind = "List";
              unique = false;
              element = f.value;
            };
          }
        else
          f
      ) r.fields;
    }
  )
);
assert rejectsOutput (
  outputMutation "RuntimeError" (
    r:
    r
    // {
      fields = map (
        f:
        if f.name == "message" then
          f
          // {
            value = {
              kind = "RecordRef";
              identity = {
                kind = "Inventory";
                coordinate = "output-schema runtime-error-port-conflict";
              };
            };
          }
        else
          f
      ) r.fields;
    }
  )
);
assert builtins.length checked.records == 29;
assert builtins.length checked.vocabularies == 14;
assert
  checked.vocabularyMap."enum StdinPolicy".members == [
    "null"
    "inherit"
  ];
assert
  checked.vocabularyMap.signal.members == [
    "TERM"
    "INT"
    "QUIT"
    "HUP"
  ];
assert make "Invocation" invocation == invocation;
assert builtins.hasContext
  (make "Invocation" (
    invocation
    // {
      executable = "${builtins.toFile "wire-context" "context"}/bin/tool";
    }
  )).executable;
assert lib.all (timeoutMs: rejects (make "Invocation" (invocation // { inherit timeoutMs; }))) [
  0
  null
  (-1)
  "1"
];
assert rejects (make "Invocation" (builtins.removeAttrs invocation [ "timeoutMs" ]));
assert rejects (make "Invocation" (invocation // { unexpected = true; }));
assert rejects (
  make "Invocation" (
    invocation
    // {
      tools = [
        "tool"
        "tool"
      ];
    }
  )
);
assert
  builtins.toJSON (make "TaskSpec" task)
  == ''{"defaultOutput":"summary","kind":"composite","serviceLifetime":"run-scoped","servicesRequired":[]}'';
assert
  builtins.toJSON (make "TaskSpec" (task // { artifactRefs = [ ]; }))
  == ''{"artifactRefs":[],"defaultOutput":"summary","kind":"composite","serviceLifetime":"run-scoped","servicesRequired":[]}'';
assert rejects (make "TaskSpec" (builtins.removeAttrs task [ "servicesRequired" ]));
assert
  make "SecretSource" {
    kind = "env-var";
    envVar = "KEY";
    path = null;
  } == {
    kind = "env-var";
    envVar = "KEY";
  };
assert rejects (
  make "SecretSource" {
    kind = "env-var";
    envVar = "KEY";
  }
);
assert rejects (
  make "Endpoint" {
    endpointId = "web";
    host = "0.0.0.0";
  }
);
assert fixture.constructors."local/NullableFixture" { value = null; } == { value = null; };
assert rejects (fixture.constructors."local/NullableFixture" { });
assert invalidField (field: field // { typo = true; });
assert invalidField (field: builtins.removeAttrs field [ "decode" ]);
assert invalidField (
  field:
  field
  // {
    decode = {
      kind = "Default";
      literal = null;
    };
  }
);
assert invalidField (
  field:
  field
  // {
    decode = {
      kind = "Required";
    };
    rustEncode = "OmitEmpty";
  }
);
assert invalidField (field: field // { rustEncode = "OmitAbsent"; });
assert invalidField (field: field // { nixEncode = "NotProduced"; });
assert invalidField (
  field:
  field
  // {
    decode = {
      kind = "Required";
    };
    nixEncode = "PreserveSupplied";
  }
);
assert invalidField (
  field:
  field
  // {
    value = {
      kind = "Integer";
      signed = false;
      bits = 8;
      nonzero = false;
    };
  }
);
assert invalidField (
  field:
  field
  // {
    decode = {
      kind = "Nullable";
    };
    value = {
      kind = "OpenJson";
      description = "Open diagnostic";
    };
  }
);
assert rejects (
  project (declarations // { records = declarations.records ++ declarations.records; })
);
assert rejects (
  project (changeRecord "Project" (record: record // { fields = record.fields ++ record.fields; }))
);
assert rejects (
  project (
    changeRecord "Project" (
      record:
      record
      // {
        identity = {
          kind = "Local";
          name = "Project";
        };
      }
    )
  )
);
assert rejects (
  project (
    changeRecord "Project" (
      record:
      record
      // {
        rust = record.rust // {
          file = "../escape.rs";
        };
      }
    )
  )
);
assert rejects (
  project (
    changeField "Project" "name" (
      field:
      field
      // {
        rust = field.rust // {
          name = "project_id";
        };
      }
    )
  )
);
assert rejects (
  project (
    changeField "Project" "name" (
      field:
      field
      // {
        value = {
          kind = "RecordRef";
          identity = {
            kind = "Local";
            name = "Missing";
          };
        };
      }
    )
  )
);
assert rejects (
  project (
    changeField "Project" "name" (
      field:
      field
      // {
        value = {
          kind = "RecordRef";
          identity = {
            kind = "Inventory";
            coordinate = "primitive Project";
          };
        };
      }
    )
  )
);
assert rejects (
  project (
    changeRecord "Project" (
      record:
      record
      // {
        rust = record.rust // {
          name = "Generator";
        };
      }
    )
  )
);
assert rejects (
  project (
    changeRecord "Project" (
      record:
      record
      // {
        rust = record.rust // {
          emission = "Borrowed";
        };
      }
    )
  )
);
assert rejects (
  project (
    changeField "Project" "name" (
      field:
      field
      // {
        rust = field.rust // {
          name = "type";
        };
      }
    )
  )
);
assert rejects (
  project (
    changeField "Project" "name" (
      field:
      field
      // {
        value = {
          kind = "NativeDomain";
          id = "BadRecord";
          description = "Cannot hide record coverage.";
          producerCheck = null;
          rustPath = [ "Project" ];
          wire = {
            kind = "RecordRef";
            identity = {
              kind = "Inventory";
              coordinate = "primitive Project";
            };
          };
        };
      }
    )
  )
);
assert rejects (
  project (
    changeField "Project" "name" (
      field:
      field
      // {
        value = {
          kind = "NativeDomain";
          id = "BadPath";
          description = "Path syntax is closed.";
          producerCheck = null;
          rustPath = [ "Vec<String>" ];
          wire = {
            kind = "Text";
          };
        };
      }
    )
  )
);
assert builtins.isString
  ((import ../meta/rust.nix { inherit lib; }) (
    check (
      changeField "Project" "name" (
        field:
        field
        // {
          value = {
            kind = "NativeDomain";
            id = "DisplayOnly";
            description = "Static projections never run native checks.";
            producerCheck = _: throw "static projection ran a native producer check";
            rustPath = [ "String" ];
            wire = {
              kind = "Text";
            };
          };
        }
      )
    )
  ))."crates/nixfied-model/src/generated/types.rs";
true
