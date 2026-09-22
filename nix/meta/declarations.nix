# Ordinary constructors shared by the two structural declaration bundles.
rec {
  text = {
    kind = "Text";
  };
  boolean = {
    kind = "Boolean";
  };
  integer = signed: bits: nonzero: {
    kind = "Integer";
    inherit signed bits nonzero;
  };
  u16 = integer false 16 false;
  u32 = integer false 32 false;
  u64 = integer false 64 false;
  i32 = integer true 32 false;
  i64 = integer true 64 false;
  nz32 = integer false 32 true;
  nz64 = integer false 64 true;
  enum = coordinate: {
    kind = "Enum";
    inherit coordinate;
  };
  list = element: {
    kind = "List";
    inherit element;
    unique = false;
  };
  unique = element: (list element) // { unique = true; };
  mapOf = value: {
    kind = "Map";
    inherit value;
  };
  inventory = coordinate: {
    kind = "Inventory";
    inherit coordinate;
  };
  local = name: {
    kind = "Local";
    inherit name;
  };
  ref = identity: {
    kind = "RecordRef";
    inherit identity;
  };
  required = {
    kind = "Required";
  };
  optional = {
    kind = "Optional";
  };
  omitted = {
    kind = "OptionalOmitted";
  };
  empty = {
    kind = "Empty";
  };
  emptyOmitted = {
    kind = "EmptyOmitted";
  };
  enumDefault = member: {
    kind = "EnumDefault";
    inherit member;
  };
  field = name: value: presence: description: {
    inherit
      name
      value
      presence
      description
      ;
    rust = { };
  };
}
