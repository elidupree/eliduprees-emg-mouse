import sys
import subprocess

while True:
    result = subprocess.run (sys.argv[1:], stderr=subprocess.PIPE)
    if result.returncode == 1 and b"X Error" in result.stderr:
        print("Crashed due to X Error, re-running")
        continue
    else:
        break