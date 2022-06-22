import numpy as np
import matplotlib.pyplot as plt
import json

with open("reports.json") as file:
    data = json.load(file)

fig, ax = plt.subplots()

for frame in data["frames"]:
    ax.plot([iteration["learning_rate"] for iteration in frame["iterations"]])

ax.set_xlabel("Iteration")
ax.set_ylabel("Learning rate")

ax.grid(True)
ax.set_yscale("log")
fig.tight_layout()

plt.show()
