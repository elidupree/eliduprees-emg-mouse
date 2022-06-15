from abc import abstractmethod, ABC

import numpy as np


class Parameters:
    def __init__(self):
        self.landmarks = None

        # the "spatial depth units per spatial horizontal unit" at 1.0 planar units away from camera center
        # units are "spatial depth units" * "planar units" / "spatial horizontal units"
        self.fov_slope = None

    def copy(self):
        result = Parameters()
        result.landmarks = self.landmarks.copy()
        result.fov_slope = self.fov_slope
        return result

    @staticmethod
    def default_from_camera(camera_landmarks):
        assert camera_landmarks.shape[1] == 2
        result = Parameters()
        result.landmarks = np.hstack([camera_landmarks, np.ones((camera_landmarks.shape[0], 1))])
        result.fov_slope = 1
        return result

    # "error is the square of the planar distance between expected and observed camera locations"
    def landmark_error(self, camera_landmark, parameter_landmark):
        px, py, pz = parameter_landmark
        cx, cy = camera_landmark
        cz = self.fov_slope
        return (px * cz / pz - cx) ** 2 + (py * cz / pz - cy) ** 2
        # return (px*cz - cx*pz)**2 + (py*cz - cy*pz)**2

    def d_error_d_landmark(self, camera_landmark, parameter_landmark):
        px, py, pz = parameter_landmark
        cx, cy = camera_landmark
        cz = self.fov_slope
        return np.array([
            2 * cz * (cz * px - cx * pz) / (pz ** 2),
            2 * cz * (cz * py - cy * pz) / (pz ** 2),
            2 * (pz * (px * cx + py * cy) - cz * (px ** 2 + py ** 2)) / (pz ** 3),
        ])

    def d_landmark_error_d_fov_slope(self, camera_landmark, parameter_landmark):
        px, py, pz = parameter_landmark
        cx, cy = camera_landmark
        cz = self.fov_slope
        return 2 * (cz * (px * cx + py * cy) - pz * (cx ** 2 + cy ** 2)) / (cz ** 3)

    def error(self, camera_landmarks):
        return sum(self.landmark_error(c, p) for c, p in zip(camera_landmarks, self.landmarks))

    def d_error_d_landmarks(self, camera_landmarks):
        return np.array([self.d_error_d_landmark(c, p) for c, p in zip(camera_landmarks, self.landmarks)])

    def d_error_d_fov_slope(self, camera_landmarks):
        return sum(self.d_landmark_error_d_fov_slope(c, p) for c, p in zip(camera_landmarks, self.landmarks))

    # def d_error_d_transformation(self, camera_landmarks, transformation_derivative_matrix):
    #     (self.landmarks @ transformation_derivative_matrix) * self.d_error_d_landmarks(camera_landmarks)

    def conformed_to(self, camera_landmarks):
        current = ParametersAnalysis(self, camera_landmarks)
        moves = [ChangeRunner(MoveHead(d)) for d in range(3)]
        rotations = [ChangeRunner(RotateHead([c for c in range(3) if c != d])) for d in range(3)]
        reshape = ChangeRunner(ReshapeHead())
        for iteration in range(100):
            # print(f"Iter {iteration}:")
            candidates = moves.copy()
            if iteration > 10:
                candidates += rotations
            if iteration > 20:
                candidates += [reshape]
            for candidate in candidates:
                current.analyze()
                current = candidate.apply(current)
            if current.error < 0.001 ** 2 * len(camera_landmarks):
                print(f"Good enough at iteration {iteration}")
                break

        return current.parameters


class ParametersAnalysis:
    def __init__(self, parameters, camera_landmarks):
        self.center_of_mass = None
        self.d_error_d_fov_slope = None
        self.d_error_d_landmarks = None
        self.parameters = parameters
        self.camera_landmarks = camera_landmarks
        self.error = self.parameters.error(self.camera_landmarks)

    def analyze(self):
        self.d_error_d_landmarks = self.parameters.d_error_d_landmarks(self.camera_landmarks)
        self.d_error_d_fov_slope = self.parameters.d_error_d_fov_slope(self.camera_landmarks)
        self.center_of_mass = np.mean(self.parameters.landmarks, axis=0)


class ParametersChange(ABC):
    @abstractmethod
    def apply(self, parameters: Parameters, learning_rate):
        """Apply this change to the given parameters, by the amount given by learning_rate"""


class ChangeRunner:
    def __init__(self, change: ParametersChange):
        self.change = change
        self.learning_rate = 0.01

    def apply(self, current: ParametersAnalysis) -> ParametersAnalysis:
        new_parameters = current.parameters.copy()
        self.change.apply(new_parameters, current, self.learning_rate)
        new = ParametersAnalysis(new_parameters, current.camera_landmarks)
        # print(f"{self.change}: {self.learning_rate:.6f}, {current.error:.6f}, {new.error:.6f}")
        if new.error < current.error:
            self.learning_rate *= 1.1
            return new
        else:
            self.learning_rate /= 2
            return current


def replace_submatrix(mat, ind1, ind2, replacement):
    for row_index, new_row in zip(ind1, replacement):
        mat[row_index, ind2] = new_row


class RotateHead(ParametersChange):
    def __init__(self, dimensions):
        self.dimensions = dimensions

    def __str__(self):
        return f"RotateHead({self.dimensions})"

    def apply(self, new_parameters: Parameters, current_analysis: ParametersAnalysis, learning_rate):
        center = current_analysis.center_of_mass

        derivative = np.zeros((3, 3))
        replace_submatrix(derivative, self.dimensions, self.dimensions, [
            [0, -1],
            [1, 0],
        ])
        d_landmarks_d_radians = (
                                        current_analysis.parameters.landmarks - center) @ derivative.transpose()
        d_error_d_radians = np.sum(current_analysis.d_error_d_landmarks * d_landmarks_d_radians)
        radians = -d_error_d_radians * learning_rate
        # print(f"RotateHead {self.dimensions}: {radians:.6f} radians")

        rotation = np.eye(3)
        replace_submatrix(rotation, self.dimensions, self.dimensions, [
            [np.cos(radians), -np.sin(radians)],
            [np.sin(radians), np.cos(radians)],
        ])
        new_parameters.landmarks -= center
        new_parameters.landmarks = new_parameters.landmarks @ rotation.transpose()
        new_parameters.landmarks += center


class MoveHead(ParametersChange):
    def __init__(self, dimension):
        self.dimension = dimension

    def __str__(self):
        return f"MoveHead({self.dimension})"

    def apply(self, new_parameters: Parameters, current_analysis: ParametersAnalysis, learning_rate):
        d_error_d_distance = np.sum(current_analysis.d_error_d_landmarks[:, self.dimension])
        distance = -d_error_d_distance * learning_rate
        # print(f"MoveHead {self.dimension}: {distance:.6f} distance")

        new_parameters.landmarks[:, self.dimension] += distance


class ReshapeHead(ParametersChange):
    def __str__(self):
        return f"ReshapeHead"

    def apply(self, new_parameters: Parameters, current_analysis: ParametersAnalysis, learning_rate):
        changes = -current_analysis.d_error_d_landmarks * learning_rate
        # print(f"ReshapeHead: {np.mean(changes):.6f} mean distance")

        new_parameters.landmarks += changes
