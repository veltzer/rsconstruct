# Coding Standards

Rules that apply to the RSB codebase and its documentation.

## Fail hard, never degrade gracefully

When something fails, it must fail the entire build. Do not try-and-fallback,
do not silently substitute defaults for missing resources, do not swallow errors.
If a processor is configured to use a file and that file does not exist, that is
an error. The user must fix their configuration or their project, not the code.

Optional features must be opt-in via explicit configuration (default off).
When the user enables a feature, all resources it requires must exist.

## Never hard-code counts of dynamic sets

Documentation and code must never state the number of processors, commands,
or any other set that changes as the project evolves. Use phrasing like
"all processors" instead of "all seven processors". Enumerating the members
of a set is acceptable; stating the cardinality is not.
