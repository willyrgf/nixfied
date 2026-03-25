# Canonical value-to-string converters.
# Two variants exist because shell env and contract/JSON contexts
# use different boolean representations.
{
  # For contract/JSON/kernel contexts: true → "true", false → "false"
  toContractString =
    value:
    if value == null then
      ""
    else if builtins.isBool value then
      if value then "true" else "false"
    else
      toString value;

  # For shell environment variables: true → "1", false → "0"
  toShellEnvString =
    value:
    if value == null then
      ""
    else if builtins.isBool value then
      if value then "1" else "0"
    else
      toString value;
}
