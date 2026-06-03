{
  hashJson = value: builtins.hashString "sha256" (builtins.toJSON value);
}
