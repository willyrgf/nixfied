{
  toFile = name: value: builtins.toFile name (builtins.toJSON value);
}
