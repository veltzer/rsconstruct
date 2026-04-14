- deps in source code design.
    A user can put "deps: url"
    in any file and we will scan for it and deduce dependency on something.

    e.g.
    in foo.py:
    // deps: data.yaml
    with open("data.yaml") as handle:
      ...

    Actually we need types of deps: runtime deps, compilation deps, ...

    This is a dependency scanner

    depenencies can be on other things, like:
      secret keys in pass
      external modules (import yaml)
      other ideas?

    A dependency scanner can deduce depenencies of various types
    for instance: a cpp dependency scanner can deduce dependency on a h file (thats compilet dependency)
    or a dependency on an external library (link dependency)
    or some other deependency in the code (on a secret key)
