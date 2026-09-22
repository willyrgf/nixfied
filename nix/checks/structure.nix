# Named independent wire/presence vectors. Expected bytes are not generated.
{ lib }:
let
  model = import ../meta/model.nix { inherit lib; };
  outputs = import ../meta/outputs.nix { inherit lib; };
  d = import ../meta/declarations.nix;
  inventory = import ../meta/inventory.nix { inherit lib; } (
    builtins.readFile ../../runtime/crates/nixfied-model/capability.txt
  );
  check =
    bundle:
    import ../meta/structure.nix { inherit lib; } (
      bundle
      // {
        inherit inventory;
        recoveryTopics = builtins.attrNames (import ../docs/topics.nix);
      }
    );
  checked = check model;
  rejects = value: !(builtins.tryEval (builtins.deepSeq value true)).success;
  changeRecord =
    bundle: name: f:
    bundle
    // {
      records = map (r: if r.rust.name == name then f r else r) bundle.records;
    };
  changeField =
    bundle: record: name: f:
    changeRecord bundle record (
      r:
      r
      // {
        fields = map (field: if field.name == name then f field else field) r.fields;
      }
    );
  badField = f: rejects (map (r: r.id) (check (changeField model "Project" "name" f)).records);
  badRecord = f: rejects (map (r: r.id) (check (changeRecord model "Project" f)).records);
  badOutput = name: f: rejects (map (r: r.id) (check (changeRecord outputs name f)).records);
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
  child = {
    identity = d.local "Child";
    description = "Nested wire fixture.";
    decoder = "RejectUnknown";
    producer = "Nix";
    rust = {
      file = "fixture.rs";
      name = "Child";
      visibility = "pub";
      emission = "Owned";
      derives = [ ];
    };
    fields = [
      (
        (d.field "value" d.text { kind = "OptionalOmitted"; } "Optional text.")
        // {
          nixEncode = "RequiredOmitAbsent";
        }
      )
    ];
  };
  nested =
    field: input:
    let
      checked = check {
        vocabularies = [ ];
        records = [
          (child // { fields = [ field ]; })
          (
            child
            // {
              identity = d.local "Parent";
              rust = child.rust // {
                name = "Parent";
              };
              fields = [
                (
                  (d.field "child" (d.ref child.identity) { kind = "Required"; } "Emitted child.")
                  // {
                    nixEncode = "RequiredPresent";
                  }
                )
              ];
            }
          )
        ];
      };
    in
    checked.constructors."local/Parent" { child = input; };
  optional = builtins.head child.fields;
  empty = optional // {
    value = d.list d.text;
    presence.kind = "EmptyOmitted";
    nixEncode = "RequiredOmitEmpty";
  };
  native = {
    kind = "NativeDomain";
    id = "DisplayOnly";
    wire = d.text;
    description = "Static checking never calls native validation.";
    producerCheck = _: throw "native predicate forced";
    rustPath = [ "String" ];
  };
  cases = {
    invocation = {
      expr = make "Invocation" invocation;
      expected = invocation;
    };
    context = {
      expr =
        builtins.hasContext
          (make "Invocation" (
            invocation // { executable = "${builtins.toFile "wire-context" "context"}/bin/tool"; }
          )).executable;
      expected = true;
    };
    timeout = {
      expr =
        builtins.all (timeoutMs: rejects (make "Invocation" (invocation // { inherit timeoutMs; })))
          [
            0
            null
            (-1)
            "1"
          ];
      expected = true;
    };
    missing = {
      expr = rejects (make "Invocation" (builtins.removeAttrs invocation [ "timeoutMs" ]));
      expected = true;
    };
    unknown = {
      expr = rejects (make "Invocation" (invocation // { extra = true; }));
      expected = true;
    };
    duplicate = {
      expr = rejects (
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
      expected = true;
    };
    task = {
      expr = builtins.toJSON (make "TaskSpec" task);
      expected = ''{"defaultOutput":"summary","kind":"composite","serviceLifetime":"run-scoped","servicesRequired":[]}'';
    };
    suppliedEmpty = {
      expr = builtins.toJSON (make "TaskSpec" (task // { artifactRefs = [ ]; }));
      expected = ''{"artifactRefs":[],"defaultOutput":"summary","kind":"composite","serviceLifetime":"run-scoped","servicesRequired":[]}'';
    };
    requiredDespiteDefault = {
      expr = rejects (make "TaskSpec" (builtins.removeAttrs task [ "servicesRequired" ]));
      expected = true;
    };
    secret = {
      expr = make "SecretSource" {
        kind = "env-var";
        envVar = "KEY";
        path = null;
      };
      expected = {
        kind = "env-var";
        envVar = "KEY";
      };
    };
    secretMissing = {
      expr = rejects (
        make "SecretSource" {
          kind = "env-var";
          envVar = "KEY";
        }
      );
      expected = true;
    };
    host = {
      expr = rejects (
        make "Endpoint" {
          endpointId = "web";
          host = "0.0.0.0";
        }
      );
      expected = true;
    };
    nestedAbsent = {
      expr = nested optional { };
      expected = {
        child = { };
      };
    };
    nestedNull = {
      expr = rejects (nested optional { value = null; });
      expected = true;
    };
    nestedEmpty = {
      expr = nested empty { };
      expected = {
        child = { };
      };
    };
    nestedExplicitEmpty = {
      expr = rejects (nested empty { value = [ ]; });
      expected = true;
    };
    nestedPresent = {
      expr = nested optional { value = "text"; };
      expected = {
        child.value = "text";
      };
    };
    nestedUnknown = {
      expr = rejects (nested optional { extra = true; });
      expected = true;
    };
    unknownMetadata = {
      expr = badField (f: f // { typo = true; });
      expected = true;
    };
    legacyDecode = {
      expr = badField (f: f // { decode.kind = "Required"; });
      expected = true;
    };
    missingPresence = {
      expr = badField (f: builtins.removeAttrs f [ "presence" ]);
      expected = true;
    };
    defaultLanguage = {
      expr = builtins.all (presence: badField (f: f // { inherit presence; })) [
        { kind = "Nullable"; }
        {
          kind = "Default";
          literal = "text";
        }
        {
          kind = "Required";
          literal = null;
        }
        { kind = "Empty"; }
        {
          kind = "EnumDefault";
          member = "text";
        }
      ];
      expected = true;
    };
    invalidEnumDefault = {
      expr = rejects (
        map (r: r.id)
          (check (changeField model "TaskSpec" "defaultOutput" (f: f // { presence.member = "missing"; })))
          .records
      );
      expected = true;
    };
    omission = {
      expr = builtins.all (nixEncode: badField (f: f // { inherit nixEncode; })) [
        "NotProduced"
        "RequiredOmitAbsent"
        "RequiredOmitEmpty"
        "PreserveSupplied"
      ];
      expected = true;
    };
    decoderEmpty = {
      expr = badField (
        f:
        f
        // {
          value = d.list d.text;
          presence.kind = "OmitEmpty";
        }
      );
      expected = true;
    };
    integer = {
      expr = badField (f: f // { value = d.integer false 8 false; });
      expected = true;
    };
    duplicateRecords = {
      expr = rejects (
        map (r: r.id) (check (model // { records = model.records ++ model.records; })).records
      );
      expected = true;
    };
    duplicateFields = {
      expr = badRecord (r: r // { fields = r.fields ++ r.fields; });
      expected = true;
    };
    localBypass = {
      expr = badRecord (r: r // { identity = d.local "Project"; });
      expected = true;
    };
    pathEscape = {
      expr = badRecord (
        r:
        r
        // {
          rust = r.rust // {
            file = "../escape.rs";
          };
        }
      );
      expected = true;
    };
    duplicateBinding = {
      expr = badRecord (
        r:
        r
        // {
          rust = r.rust // {
            name = "Generator";
          };
        }
      );
      expected = true;
    };
    duplicateRustField = {
      expr = badField (f: f // { rust.name = "project_id"; });
      expected = true;
    };
    dangling = {
      expr = badField (f: f // { value = d.ref (d.local "Missing"); });
      expected = true;
    };
    cycle = {
      expr = badField (f: f // { value = d.ref (d.inventory "primitive Project"); });
      expected = true;
    };
    borrowedDecoder = {
      expr = badRecord (
        r:
        r
        // {
          rust = r.rust // {
            emission = "Borrowed";
          };
        }
      );
      expected = true;
    };
    keyword = {
      expr = badField (f: f // { rust.name = "type"; });
      expected = true;
    };
    nativeRecord = {
      expr = badField (
        f:
        f
        // {
          value = native // {
            wire = d.ref (d.inventory "primitive Project");
          };
        }
      );
      expected = true;
    };
    nativePath = {
      expr = badField (
        f:
        f
        // {
          value = native // {
            rustPath = [ "Vec<String>" ];
          };
        }
      );
      expected = true;
    };
    nativeLazy = {
      expr =
        builtins.isString
          ((import ../meta/rust.nix { inherit lib; }) (
            check (changeField model "Project" "name" (f: f // { value = native; }))
          ))."crates/nixfied-model/src/generated/types.rs";
      expected = true;
    };
    outputConstructors = {
      expr = (check outputs).constructors;
      expected = { };
    };
    outputFieldProducer = {
      expr = badOutput "TaskRun" (
        r: r // { fields = map (f: f // { nixEncode = "RequiredPresent"; }) r.fields; }
      );
      expected = true;
    };
    borrowedOutputDecoder = {
      expr = badOutput "ProjectionDiagnostic" (r: r // { decoder = "IgnoreUnknown"; });
      expected = true;
    };
    borrowedBox = {
      expr = badOutput "PortConflictDetails" (
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
      );
      expected = true;
    };
    ownedBorrowedRef = {
      expr = badOutput "RuntimeError" (
        r:
        r
        // {
          fields = map (
            f:
            if f.name == "message" then
              f // { value = d.ref (d.inventory "output-schema runtime-error-projection"); }
            else
              f
          ) r.fields;
        }
      );
      expected = true;
    };
    borrowedCollection = {
      expr = badOutput "PortConflictDetails" (
        r:
        r
        // {
          fields = map (f: if f.name == "endpoint" then f // { value = d.list f.value; } else f) r.fields;
        }
      );
      expected = true;
    };
    memberKeyRef = {
      expr = badOutput "RuntimeError" (
        r:
        r
        // {
          fields = map (
            f:
            if f.name == "message" then
              f // { value = d.ref (d.inventory "output-schema runtime-error-port-conflict"); }
            else
              f
          ) r.fields;
        }
      );
      expected = true;
    };
    errorAnnotations = {
      expr =
        builtins.all
          (
            annotations:
            rejects (
              map (v: v.coordinate)
                (check (
                  outputs
                  // {
                    vocabularies = map (
                      v: if v.coordinate == "error-code" then v // { inherit annotations; } else v
                    ) outputs.vocabularies;
                  }
                )).vocabularies
            )
          )
          [
            { }
            {
              TASK_FAILED = {
                description = "Known";
                recoveryTopic = "missing";
              };
            }
          ];
      expected = true;
    };
  };
  failures = lib.debug.runTests (
    lib.mapAttrs' (name: value: lib.nameValuePair "test_${name}" value) cases
  );
in
assert lib.assertMsg (failures == [ ]) (builtins.toJSON failures);
true
