This is rust build tool. with incrementatl build and checksums.

The main commands are:

```bash
$ rsb build
```

This issus an incremental build.

```bash
$ rsb clean
```

This issues a full clean.

We will use the best command line parsing engine.


First feature - templates

convention over configuration.
Every file in templates/{X}.tera will create a file called {X} (no templates prefix and no .tera suffix)
using the tera templating engine.

There will be our own function in tera (load_python) that will load python config files from any path
The config files will usually be in a folder config beside templates.
