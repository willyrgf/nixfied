{ lib }:
let
  t = lib.types;
in
let
  contractNodeOptions = rec {
    contractNodeType = t.submodule {
      options = contractNodeOptions;
    };

    kind = lib.mkOption {
      type = t.enum [
        "bool"
        "string"
        "integer"
        "number"
        "null"
        "literal"
        "enum"
        "list"
        "map"
        "record"
        "union"
        "taggedUnion"
        "ref"
      ];
      default = "record";
    };

    doc = lib.mkOption {
      type = t.nullOr t.str;
      default = null;
    };

    closed = lib.mkOption {
      type = t.bool;
      default = true;
    };

    fields = lib.mkOption {
      type = t.nullOr (t.attrsOf contractNodeType);
      default = null;
    };

    options = lib.mkOption {
      type = t.nullOr (t.listOf contractNodeType);
      default = null;
    };

    elem = lib.mkOption {
      type = t.nullOr contractNodeType;
      default = null;
    };

    key = lib.mkOption {
      type = t.nullOr contractNodeType;
      default = null;
    };

    value = lib.mkOption {
      type = t.nullOr contractNodeType;
      default = null;
    };

    minimum = lib.mkOption {
      type = t.nullOr t.int;
      default = null;
    };

    maximum = lib.mkOption {
      type = t.nullOr t.int;
      default = null;
    };

    minItems = lib.mkOption {
      type = t.nullOr t.int;
      default = null;
    };

    maxItems = lib.mkOption {
      type = t.nullOr t.int;
      default = null;
    };

    values = lib.mkOption {
      type = t.nullOr (
        t.listOf (
          t.oneOf [
            t.str
            t.int
            t.bool
            t.float
          ]
        )
      );
      default = null;
    };

    tag = lib.mkOption {
      type = t.nullOr t.str;
      default = null;
    };

    variants = lib.mkOption {
      type = t.nullOr (t.attrsOf contractNodeType);
      default = null;
    };

    valueLiteral = lib.mkOption {
      type = t.nullOr (
        t.oneOf [
          t.str
          t.int
          t.bool
          t.float
          t.null
        ]
      );
      default = null;
      description = "Literal value for contracts with kind = literal.";
    };

    pattern = lib.mkOption {
      type = t.nullOr t.str;
      default = null;
    };

    minLength = lib.mkOption {
      type = t.nullOr t.int;
      default = null;
    };

    maxLength = lib.mkOption {
      type = t.nullOr t.int;
      default = null;
    };

    name = lib.mkOption {
      type = t.nullOr t.str;
      default = null;
    };
  };

  contractDefinition = t.submodule {
    options = contractNodeOptions;
  };
in
{
  inherit contractDefinition;
}
