import json

import cv2
import mediapipe as mp
from datetime import datetime, timedelta
from collections import deque
# import threading
# import tkinter as tk
from skmultiflow.trees import iSOUPTreeRegressor
import numpy as np
import pyautogui
import win32api
from formulas import Parameters
import sys


def eprint(*args, **kwargs):
    print(*args, file=sys.stderr, **kwargs)


mp_drawing = mp.solutions.drawing_utils
mp_drawing_styles = mp.solutions.drawing_styles
mp_face_mesh = mp.solutions.face_mesh

screen_width, screen_height = pyautogui.size()


def face_loop():
    start = datetime.now()
    cap = cv2.VideoCapture(0, cv2.CAP_DSHOW)
    cap.set(cv2.CAP_PROP_FRAME_WIDTH, 640)
    cap.set(cv2.CAP_PROP_FRAME_HEIGHT, 480)
    end = datetime.now()
    if not cap.isOpened():
        eprint("Cannot open camera")
        exit()
    eprint("opened camera", (end - start))
    queue = deque()

    # isoup_tree = iSOUPTreeRegressor()
    # mouse_pressed = win32api.GetKeyState(0x01)
    # last_shoved_to = None
    # last_moved_to = None
    # last_moved_time = datetime.now()

    frames = 0
    start = datetime.now()

    # current_parameters = None

    with mp_face_mesh.FaceMesh(
            static_image_mode=False,
            max_num_faces=1,
            refine_landmarks=True,
            min_detection_confidence=0.5,
            min_tracking_confidence=0.5) as face_mesh:
        while cap.isOpened():
            success, image = cap.read()
            if not success:
                eprint("skipped failed read")
                continue

            frames += 1

            # eprint("got frame", image.shape)
            image.flags.writeable = False
            image = cv2.cvtColor(image, cv2.COLOR_BGR2RGB)
            results = face_mesh.process(image)

            image.flags.writeable = True
            image = cv2.cvtColor(image, cv2.COLOR_RGB2BGR)

            if results.multi_face_landmarks:
                landmarks = [[p.x - 0.5, p.y - 0.5] for p in results.multi_face_landmarks[0].landmark]
                json.dump(landmarks, sys.stdout)
                print()

                # landmarks = np.array([[p.x - 0.5, p.y - 0.5] for p in results.multi_face_landmarks[0].landmark])
                # landmarks = landmarks[[4, 152, 263, 33, 287, 57]]
                # if current_parameters is None:
                #     current_parameters = Parameters.default_from_camera(landmarks)
                # else:
                #     current_parameters = current_parameters.conformed_to(landmarks)
                # X = np.array([[a for p in results.multi_face_landmarks[0].landmark for a in [p.x, p.y]]])
                # mouse_pos = list(pyautogui.position())
                # y = np.array([mouse_pos])
                # y_pred = isoup_tree.predict(X)[0]
                # eprint(mouse_pos, y_pred)
                #
                # if mouse_pos != last_moved_to and mouse_pos != last_shoved_to:
                #     last_moved_to = mouse_pos
                #     last_moved_time = datetime.now()
                #
                # mouse_pressed_new = win32api.GetKeyState(0x01)
                # if mouse_pressed_new and not mouse_pressed:
                #     isoup_tree.partial_fit(X, y)
                # mouse_pressed = mouse_pressed_new
                #
                # if len(y_pred) == 2 and last_moved_time + timedelta(seconds=1) < datetime.now() and y_pred[0] > 0 and y_pred[0] < screen_width:
                #     last_shoved_to = [round(a) for a in y_pred]
                #     pyautogui.moveTo(*last_shoved_to)

                for face_landmarks in results.multi_face_landmarks:
                    mp_drawing.draw_landmarks(
                        image=image,
                        landmark_list=face_landmarks,
                        connections=mp_face_mesh.FACEMESH_TESSELATION,
                        landmark_drawing_spec=None,
                        connection_drawing_spec=mp_drawing_styles
                            .get_default_face_mesh_tesselation_style())
                    mp_drawing.draw_landmarks(
                        image=image,
                        landmark_list=face_landmarks,
                        connections=mp_face_mesh.FACEMESH_CONTOURS,
                        landmark_drawing_spec=None,
                        connection_drawing_spec=mp_drawing_styles
                            .get_default_face_mesh_contours_style())
                    mp_drawing.draw_landmarks(
                        image=image,
                        landmark_list=face_landmarks,
                        connections=mp_face_mesh.FACEMESH_IRISES,
                        landmark_drawing_spec=None,
                        connection_drawing_spec=mp_drawing_styles
                            .get_default_face_mesh_iris_connections_style())

            queue.append(image)

            eprint(f"FPS: {frames / (datetime.now() - start).total_seconds():.1f}")
            if len(queue) == 1:  # 60:
                cv2.imshow('MediaPipe Face Mesh', cv2.flip(queue.popleft(), 1))
            if cv2.waitKey(2) & 0xFF == 27:
                break

    cap.release()


# def tk_thread():
#     root = tk.Tk()
#     root.overrideredirect(True)
#     root.geometry("+250+250")
#     root.wm_attributes("-topmost", True)
#     root.wm_attributes("-disabled", True)
#     root.wm_attributes("-transparentcolor", "white")
#     root.mainloop()
#     root.quit()
#
#
# t = threading.Thread(target=tk_thread)
# t.start()

# import cProfile
#
# cProfile.run("face_loop()")
face_loop()
