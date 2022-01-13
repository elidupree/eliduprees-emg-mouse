import os
import json
from dotenv import load_dotenv

load_dotenv()


s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.connect((os.environ["EMG_SERVER_IP"], os.environ["EMG_SERVER_PORT"]))

for line in s.makefile():
    print(json.loads(line))
