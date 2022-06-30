import numpy as np
import matplotlib.pyplot as plt
import json

with open("reports.json") as file:
    data = json.load(file)

fig, ax = plt.subplots()

frames_start = 103
frames_len = 1
frames = data['frames'][frames_start:frames_start + frames_len]
print(f"plotting {len(frames)} frames")

for frame in frames:
    ax.plot([iteration["learning_rate"] for iteration in frame["iterations"]])
    if "optimal_learning_rate" in frame["iterations"][0]:
        ax.plot([iteration["optimal_learning_rate"] for iteration in frame["iterations"]])
    ax.plot([iteration["observed_learning_ratio"] for iteration in frame["iterations"]])

    # ax.plot([iteration["loss"] for iteration in frame["iterations"]])
    for descent_kind_index, color in enumerate(["red", "green", "blue", "purple"]):
        ax.plot(
            [iteration["proposed_descent_kind_magnitudes"][descent_kind_index] for iteration in frame["iterations"]],
            c=color)

ax.set_xlabel("Iteration")
ax.set_ylabel("Learning rate")

ax.grid(True)
ax.set_yscale("log")
fig.tight_layout()

plt.show()
