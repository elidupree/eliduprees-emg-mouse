import os
import socket
import json
import pyglet
import pyautogui
from dotenv import load_dotenv
import matplotlib.pyplot as plt
import numpy as np
from datetime import datetime


load_dotenv()

click_sound = pyglet.media.load("media/click.wav", streaming = False)
unclick_sound = pyglet.media.load("media/unclick.wav", streaming = False)

s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.connect((os.environ["EMG_SERVER_IP"], int(os.environ["EMG_SERVER_PORT"])))

mouse_pressed = False
click_threshold = 500
unclick_threshold = 200

memory_count = 200
left_button_memory = [0]* memory_count

# plt.style.use('ggplot')
# plt.ion()
# fig = plt.figure(figsize=(13,6))
# ax = fig.add_subplot(111)
# left_button_line, = ax.plot(list(range (memory_count)),left_button_memory,'-o')
# plt.ylabel('mV I think')
# plt.ylim([0, 2500])
# plt.show()

total_inputs = 0
start = datetime.now()

for line in s.makefile():
    data = json.loads(line)
    print(data)
    if mouse_pressed:
        if data["left_button"] < unclick_threshold:
            #pyautogui.mouseUp()
            unclick_sound.play()
            mouse_pressed = False
    else:
        if data["left_button"] > click_threshold:
            #pyautogui.mouseDown()
            click_sound.play()
            mouse_pressed = True

    total_inputs += 1
    now = datetime.now()
    #print (f"{total_inputs}: {total_inputs/(now- start).total_seconds()}")

    left_button_memory = left_button_memory[1:] + [data["left_button"]]
    # if total_inputs % 100 == 0:
    #     left_button_line.set_ydata(left_button_memory)
    #     plt.pause(0.001)
