let
  uniquePreserveOrder =
    list:
    builtins.foldl' (acc: value: if builtins.elem value acc then acc else acc ++ [ value ]) [ ] list;

  uniqueSorted =
    values:
    let
      unique = lst: builtins.foldl' (acc: v: if builtins.elem v acc then acc else acc ++ [ v ]) [ ] lst;
    in
    builtins.sort builtins.lessThan (unique values);
in
{
  inherit uniquePreserveOrder uniqueSorted;

  uniqueNonEmptyPreserveOrder =
    list: uniquePreserveOrder (builtins.filter (value: value != null && value != "") list);
}
