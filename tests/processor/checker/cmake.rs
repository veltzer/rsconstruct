test_checker!(cmake, tool: "cmakelint", processor: "processor.checker.cmake",
    files: [("CMakeLists.txt", "cmake_minimum_required(VERSION 3.10)\nproject(demo)\n")]);
