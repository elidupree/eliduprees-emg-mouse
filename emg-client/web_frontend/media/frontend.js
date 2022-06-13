
const canvas = document.getElementById ("canvas")
const context = canvas.getContext ("2d")
const followers_element = document.getElementById ("followers");
const variables_element = document.getElementById ("variables");
const enabled_checkbox = document.getElementById ("enabled_checkbox");

let socket = null

function send(variant, contents) {
  if (socket !== null && socket.readyState == 1) {
    socket.send(JSON.stringify({[variant]: contents}))
  }
}

enabled_checkbox.addEventListener("click", e => {
  send("SetEnabled", enabled_checkbox.checked);
});

const message_handlers = {}
let recent_frames = [];

message_handlers.Initialize = ({ enabled, variables }) => {
    enabled_checkbox.checked = enabled;
    context.clearRect(0, 0, canvas.width, canvas.height);
    recent_frames = [];
    latest_received_frame_time = 0;
    latest_drawn_frame_time = 0;
    followers_element.innerHTML = "";
    variables_element.innerHTML = "";


  for (const [name, value] of Object.entries(variables)) {
    const id = `variables_editor_${name}`;
    if (!document.getElementById(id)) {
      const new_container = document.createElement("div");
      new_container.id = id;
      new_container.className = "variables_editor_entry";
      const number_input = document.createElement("input");
      const range_input = document.createElement("input");

      const update_range_input = val => {
        range_input.value = val;
        if (val > 0) {
          range_input.setAttribute("min", val * 0.01);
          range_input.setAttribute("max", val * 5);
        } else {
          range_input.setAttribute("min", val * 5);
          range_input.setAttribute("max", -val * 5);
        }
        range_input.setAttribute("step", val * 0.01);
      }

      number_input.id = `${id}_number`;
      number_input.setAttribute("type", "number");
      number_input.value = value;
      number_input.addEventListener("input", (event) => {
        send("SetVariable", [name, number_input.valueAsNumber]);
        update_range_input(number_input.valueAsNumber);
      });

      range_input.id = `${id}_range`;
      range_input.setAttribute("type", "range");
      update_range_input(value);
      range_input.addEventListener("input", (event) => {
        send("SetVariable", [name, range_input.valueAsNumber]);
        number_input.value = range_input.valueAsNumber;
      });

      const label = document.createElement("label");
      label.setAttribute("for", number_input.id);
      label.textContent = name;

      new_container.appendChild(range_input);
      new_container.appendChild(number_input);
      new_container.appendChild(label);
      variables_element.appendChild(new_container);
    }
  }
//    for (const server of recent_frames) {
//      for (const signal of server) {
//        for (const frame_kind of Object.values(signal)) {
//          frame_kind.length = 0;
//        }
//      }
//    }
}

function new_signal_frames() {
  return {
    activity: [],
    frequencies: [],
  }
}
function new_server_frames() {
  return {
    latest_received_frame_time: 0,
    latest_drawn_frame_time: 0,
    signals: [
      new_signal_frames(),
      new_signal_frames(),
      new_signal_frames(),
      new_signal_frames(),
    ]
  }
}
function server_frames (server_index) {
    while (recent_frames.length <= server_index) {
      recent_frames.push(new_server_frames());
    }
    return recent_frames [server_index];
}
function add_fresh_frames(kind, server_index, frames) {
  const server = server_frames(server_index);
  server.signals.forEach(function(signal, signal_index) {
    for (const frame of frames[signal_index]) {
      signal[kind].push(frame);
      server.latest_received_frame_time = Math.max(latest_received_frame_time, frame.time);
    }
  });
}

message_handlers.NewHistoryFrames = ({ server_index, frames }) => {
    add_fresh_frames("activity",server_index, frames);
}

message_handlers.NewFrequenciesFrames = ({ server_index, frames }) => {
    add_fresh_frames("frequencies",server_index, frames);
}

message_handlers.UpdateFollower = ({ name, latest_move_time }) => {
    const id = "follower_" + name;
    let element = document.getElementById (id);
    if (!element) {
      element = document.createElement("div");
      element.id = id;
      followers_element.appendChild(element);
    }
    const text = name+": "+latest_move_time;
    if (element.innerText != text) {
      element.innerText = text;
    }
}

function connect() {
    if (socket) { socket.close() }
    socket = new WebSocket(`ws://${location.host}/session`)

    socket.onopen = () => {
      console.log('Connected')
    }

    socket.onmessage = (ev) => {
      //console.log('Received: ' + ev.data)
      const message = JSON.parse (ev.data)
      //console.log('Received: ', message)
      for (const [k,v] of Object.entries(message)) {
        message_handlers[k](v);
      }
    }

    socket.onclose = () => {
      console.log('Disconnected')
    }
}



function update_subcanvas({
    canvas,context,
    left,top,right, bottom,
    start_time, stop_time,
    draw,
    line_color, frequency_color}) {
  const width = right - left;
  const height = bottom - top;
  const canvas_duration = 0.8;
  const time_duration = stop_time - start_time;
  const start_integer = Math.round (start_time * width / canvas_duration);
  const stop_integer = Math.round (stop_time * width / canvas_duration);
  const new_area_width = stop_integer - start_integer;
  const old_area_width = width - new_area_width;
  let clip_left;
  context.save();

  // We want this to be equivalent to redrawing everything on every frame, but that's inefficient.
  // Fortunately, that's equivalent to moving everything to the left and then drawing only the new stuff.
  // We need to move by an exact integer so that the pixel values are maintained exactly.
  //
  // The other issue is, when we draw the new stuff, it might spill into the old area.
  // So we set a clipping path so that we can spill as much as we need to.
  if (old_area_width < 0) {
    context.clearRect(left, top, width, height);
    context.beginPath();
    clip_left = left;
    context.rect(clip_left, top, width, height);
    context.clip();
  } else {
    const data = context.getImageData(
      left + new_area_width, top, old_area_width, height,
    )
    context.clearRect(left, top, width, height);
    context.putImageData(data, left, top)
    context.beginPath();
    // hack - be able to catch up when old frequency frames come in (TODO: make this less hacky)
    const extra = Math.ceil(0.01 * width / canvas_duration);
    clip_left = left + old_area_width - extra;
    context.rect(clip_left, top, new_area_width + extra, height);
    context.clip();
  }

  // NOT relative to stop_time, but to stop_integer, so that the canvas position doesn't
  // vary by subpixel distances:
  function x_fractional(time) {
    const global = time * width / canvas_duration;
    return right + (global - stop_integer);
  }
  function x_integer(time) {
    const integer = Math.round(time * width / canvas_duration);
    return right + (integer - stop_integer);
  }
  function y_fractional(fraction) {
    return top + fraction * height;
  }
  function y_integer(fraction) {
    return Math.round(top + fraction * height);
  }

  draw({clip_left, x_fractional, x_integer, y_fractional, y_integer});

  context.restore();
}

const activity_colors = ["#888800", "#880088", "#000000", "#008888"];
const freq_colors = [[1, 15, 0], [15, 0, 1], [5, 1, 0], [0, 2, 15]];

function update_canvas() {
  recent_frames.forEach(function(server, server_index) {
    if (server.latest_drawn_frame_time == server.latest_received_frame_time) {
      return;
    }

    function x1(i) {
      return Math.round((i + server_index * server.signals.length) * canvas.width / (server.signals.length * recent_frames.length))
    }
    server.signals.forEach(function(signal, signal_index) {
      update_subcanvas({
        canvas, context,
        left: x1(signal_index),
        right: x1(signal_index+1),
        top: 0,
        bottom: canvas.height / 2,
        start_time: server.latest_drawn_frame_time,
        stop_time: server.latest_received_frame_time,
        draw: ({clip_left, x_fractional, y_fractional}) => {
          const frames = signal.activity;
          for (let i = frames.length - 1; i >= 0; i--) {
            if (x_fractional(frames[i].time) < clip_left - 2) {
              frames.splice(0, i);
              break;
            }
          }

          function print_line (key) {
              context.beginPath();
              frames.forEach((frame, frame_index) => {
                const x = x_fractional(frame.time);
                let y = frame[key];
                //y = (-Math.log(y * 0.99 + 0.01) / Math.log(0.01) + 1)
                y = y / 10;
                y = y_fractional(y);

                if (frame_index == 0) {
                  context.moveTo(x,y)
                } else {
                  context.lineTo(x,y);
                }
              });
              context.stroke();
          }

          context.strokeStyle = activity_colors[signal_index];
          print_line ("value");

          context.strokeStyle = "#88ff88";
          print_line ("activity_threshold");

          context.strokeStyle = "#ff8888";
          print_line ("too_much_threshold");
        }
      });

      const num_frequency_sections = server.signals[0].frequencies[0] ? server.signals[0].frequencies[0].values.length : 0;

      function x2(j) {
        return x1(signal_index + j / num_frequency_sections)
      }
      const fc = freq_colors[signal_index];
      for (let j = 0; j < num_frequency_sections; j++) {
        update_subcanvas({
          canvas, context,
          left: x2(j),
          right: x2(j+1),
          top: canvas.height / 2,
          bottom: canvas.height,
          start_time: server.latest_drawn_frame_time,
          stop_time: server.latest_received_frame_time,
          draw: ({clip_left, x_integer, y_integer}) => {
            const frames = signal.frequencies;
            for (let i = frames.length - 1; i >= 0; i--) {
              if (x_integer(frames[i].time) <= clip_left) {
                frames.splice(0, i);
                break;
              }
            }
            frames.forEach((frame) => {
                frame.values[j].forEach((intensity, frequency_index) => {
                    const right = x_integer(frame.time);
                    const left = x_integer(frame.time - 0.01);
                    const top = y_integer(frequency_index / frame.values[j].length);
                    const bottom = y_integer((frequency_index + 1) / frame.values[j].length);
                    const ic = 255 * intensity;
                    context.fillStyle = `rgb(
                        ${Math.floor(ic * fc[0])},
                        ${Math.floor(ic * fc[1])},
                        ${Math.floor(ic * fc[2])})`;
                    context.fillRect(
                      left,
                      top,
                      right-left,
                      bottom-top);
                })
            })
          }
        });
      }
    });

    server.latest_drawn_frame_time = server.latest_received_frame_time;
  });
}

function frame() {
  requestAnimationFrame (frame);

  if (!socket || socket.readyState == 3) {
    connect();
  }
  update_canvas();
}

frame();
