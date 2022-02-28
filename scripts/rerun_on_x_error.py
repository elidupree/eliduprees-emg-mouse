import sys
import subprocess

while True:
    child = subprocess.Popen(sys.argv[1:], stderr=subprocess.PIPE)
    stderr_chunks = []
    while True:
        data = child.stderr.read(4096)
        stderr_chunks.append (data)
        sys.stderr.buffer.write(data)
        sys.stderr.flush()
        if not data:
            break

    #print("\nChild stderr closed.")
    child.wait()
    #print("Child exited.")
    if child.returncode == 1 and b"X Error" in b"".join(stderr_chunks):
        print("Crashed due to X Error, re-running...")
        continue
    else:
        break