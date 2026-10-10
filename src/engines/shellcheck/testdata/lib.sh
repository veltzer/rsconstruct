#!/bin/bash
# Sourced by other fixtures; its own findings appear with check_sourced.

LIB_VALUE=1
lib_unused=2
lib_greet() {
	echo Hello $1
}
echo $lib_undefined
