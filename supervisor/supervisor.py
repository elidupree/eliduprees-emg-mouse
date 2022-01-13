import os
import socket
import json
import pyglet
import pyautogui
from dotenv import load_dotenv

load_dotenv()

click_sound = pyglet.media.load("media/click.wav", streaming = False)
unclick_sound = pyglet.media.load("media/unclick.wav", streaming = False)

s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.connect((os.environ["EMG_SERVER_IP"], int(os.environ["EMG_SERVER_PORT"])))

mouse_pressed = False
click_threshold = 500
unclick_threshold = 200

for line in s.makefile():
    data = json.loads(line)
    print(data)
    if mouse_pressed:
        if data["left_button"] < unclick_threshold:
            pyautogui.mouseUp()
            unclick_sound.play()
            mouse_pressed = False
    else:
        if data["left_button"] > click_threshold:
            pyautogui.mouseDown()
            click_sound.play()
            mouse_pressed = True
