{ lib }:
{
  normalizeToken =
    value: lib.toUpper (lib.replaceStrings [ "-" "." ":" "/" " " ] [ "_" "_" "_" "_" "_" ] value);
}
