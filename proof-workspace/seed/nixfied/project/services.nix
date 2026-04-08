{ conf }:
{
  config = {
    nixfied.services = conf.services or { };
  };
}
