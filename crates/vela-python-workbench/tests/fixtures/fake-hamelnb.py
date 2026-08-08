#!/usr/bin/env python3
import json
import subprocess
import sys
import time

args = sys.argv[1:]
code_file_index = args.index("--code-file") + 1
port_index = args.index("--port") + 1
path_index = args.index("--path") + 1
with open(args[code_file_index], encoding="utf-8") as code_file:
    source = code_file.read()

if source == "__vela_test_sleep__":
    time.sleep(30)
elif source == "__vela_test_inherited_pipe__":
    subprocess.Popen([sys.executable, "-c", "import time; time.sleep(0.25)"])
    print('{"status":"ok"}')
elif source == "__vela_test_stdout_overflow__":
    print("x" * 4096)
elif source == "__vela_test_stderr_overflow__":
    print("x" * 4096, file=sys.stderr)
    raise SystemExit(2)
elif source == "__vela_test_continuous_output__":
    while True:
        sys.stdout.write("x" * 8192)
        sys.stdout.flush()
else:
    print(json.dumps({
        "status": "ok",
        "transport": "websocket",
        "observed_code": source,
        "source_exposed_in_argv": source in args,
        "observed_port": args[port_index],
        "observed_path": args[path_index],
    }, separators=(",", ":")))
