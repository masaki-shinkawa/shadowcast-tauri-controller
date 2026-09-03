import { useCallback, useEffect, useRef, useState } from "react";
import {
  connectManualController,
  disconnectManualController,
  getManualControllerStatus,
  type ManualControllerButton,
  type ManualControllerStatus,
  type ManualControllerStick,
  neutralizeManualController,
  setManualControllerButton,
  setManualControllerStick,
} from "../lib/tauri";

const INITIAL_STATUS: ManualControllerStatus = {
  state: "disconnected",
  port: null,
  availablePorts: [],
  error: null,
};

interface VirtualControllerProps {
  scenarioRunning: boolean;
  onConnectionChange?: (connected: boolean) => void;
}

interface ControllerButtonProps {
  button: ManualControllerButton;
  label: string;
  disabled: boolean;
  className?: string;
  onChange: (button: ManualControllerButton, pressed: boolean) => void;
}

function ControllerButton({
  button,
  label,
  disabled,
  className = "",
  onChange,
}: ControllerButtonProps) {
  const [pressed, setPressed] = useState(false);

  const release = useCallback(() => {
    if (!pressed) return;
    setPressed(false);
    onChange(button, false);
  }, [button, onChange, pressed]);

  return (
    <button
      type="button"
      className={`pad-button ${pressed ? "pad-button--pressed" : ""} ${className}`}
      aria-label={label}
      aria-pressed={pressed}
      disabled={disabled}
      onContextMenu={(event) => event.preventDefault()}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        setPressed(true);
        onChange(button, true);
      }}
      onPointerUp={release}
      onPointerCancel={release}
      onLostPointerCapture={release}
    >
      {label}
    </button>
  );
}

interface AnalogStickProps {
  stick: ManualControllerStick;
  label: string;
  disabled: boolean;
  onChange: (stick: ManualControllerStick, x: number, y: number) => void;
}

function AnalogStick({ stick, label, disabled, onChange }: AnalogStickProps) {
  const surfaceRef = useRef<HTMLFieldSetElement>(null);
  const activePointer = useRef<number | null>(null);
  const [position, setPosition] = useState({ x: 0, y: 0 });

  const updateFromPointer = useCallback(
    (clientX: number, clientY: number) => {
      const surface = surfaceRef.current;
      if (!surface) return;
      const bounds = surface.getBoundingClientRect();
      const radius = bounds.width / 2;
      let x = (clientX - (bounds.left + radius)) / radius;
      let y = (clientY - (bounds.top + radius)) / radius;
      const magnitude = Math.hypot(x, y);
      if (magnitude > 1) {
        x /= magnitude;
        y /= magnitude;
      }
      setPosition({ x, y });
      onChange(stick, Math.round(0x800 + x * 0x600), Math.round(0x800 - y * 0x600));
    },
    [onChange, stick],
  );

  const release = useCallback(() => {
    if (activePointer.current === null) return;
    activePointer.current = null;
    setPosition({ x: 0, y: 0 });
    onChange(stick, 0x800, 0x800);
  }, [onChange, stick]);

  return (
    <div className="analog-control">
      <fieldset
        ref={surfaceRef}
        className={`analog-stick ${disabled ? "analog-stick--disabled" : ""}`}
        tabIndex={disabled ? -1 : 0}
        onContextMenu={(event) => event.preventDefault()}
        onPointerDown={(event) => {
          if (disabled || event.button !== 0) return;
          event.preventDefault();
          activePointer.current = event.pointerId;
          event.currentTarget.setPointerCapture(event.pointerId);
          updateFromPointer(event.clientX, event.clientY);
        }}
        onPointerMove={(event) => {
          if (activePointer.current !== event.pointerId) return;
          updateFromPointer(event.clientX, event.clientY);
        }}
        onPointerUp={release}
        onPointerCancel={release}
        onLostPointerCapture={release}
      >
        <legend className="visually-hidden">{label} analog stick</legend>
        <span className="analog-axis analog-axis--horizontal" />
        <span className="analog-axis analog-axis--vertical" />
        <span
          className="analog-thumb"
          style={{ transform: `translate(${position.x * 26}px, ${position.y * 26}px)` }}
        />
      </fieldset>
      <span>{label}</span>
    </div>
  );
}

export function VirtualController({ scenarioRunning, onConnectionChange }: VirtualControllerProps) {
  const [status, setStatus] = useState(INITIAL_STATUS);
  const [selectedPort, setSelectedPort] = useState("COM3");
  const [busy, setBusy] = useState(false);
  const [inputError, setInputError] = useState<string | null>(null);
  const commandQueue = useRef<Promise<void>>(Promise.resolve());
  const connected = status.state === "connected";
  const controlsDisabled = !connected || busy;

  const applyStatus = useCallback(
    (next: ManualControllerStatus) => {
      setStatus(next);
      if (next.port) setSelectedPort(next.port);
      else if (next.availablePorts.length > 0) setSelectedPort(next.availablePorts[0]);
      onConnectionChange?.(next.state === "connected");
    },
    [onConnectionChange],
  );

  const refresh = useCallback(async () => {
    try {
      applyStatus(await getManualControllerStatus());
    } catch (error) {
      setInputError(String(error));
    }
  }, [applyStatus]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const enqueue = useCallback(
    (command: () => Promise<void>) => {
      commandQueue.current = commandQueue.current
        .catch(() => undefined)
        .then(command)
        .catch((error) => {
          setInputError(String(error));
          void refresh();
        });
    },
    [refresh],
  );

  useEffect(() => {
    const neutralize = () => {
      if (connected) enqueue(neutralizeManualController);
    };
    const onVisibilityChange = () => {
      if (document.hidden) neutralize();
    };
    window.addEventListener("blur", neutralize);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.removeEventListener("blur", neutralize);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [connected, enqueue]);

  const handleConnect = async () => {
    setBusy(true);
    setInputError(null);
    try {
      applyStatus(await connectManualController(selectedPort));
    } catch (error) {
      setInputError(String(error));
      await refresh();
    } finally {
      setBusy(false);
    }
  };

  const handleDisconnect = async () => {
    setBusy(true);
    setInputError(null);
    try {
      await commandQueue.current.catch(() => undefined);
      applyStatus(await disconnectManualController());
    } catch (error) {
      setInputError(String(error));
      await refresh();
    } finally {
      setBusy(false);
    }
  };

  const handleButton = useCallback(
    (button: ManualControllerButton, pressed: boolean) => {
      enqueue(() => setManualControllerButton(button, pressed));
    },
    [enqueue],
  );

  const handleStick = useCallback(
    (stick: ManualControllerStick, x: number, y: number) => {
      enqueue(() => setManualControllerStick(stick, x, y));
    },
    [enqueue],
  );

  const buttonProps = { disabled: controlsDisabled, onChange: handleButton };

  return (
    <section className="controller-panel" aria-labelledby="controller-title">
      <div className="controller-heading">
        <div>
          <span className="eyebrow">MOUSE INPUT</span>
          <h2 id="controller-title">Virtual controller</h2>
        </div>
        <div className="controller-connection">
          <span className={`controller-state controller-state--${status.state}`}>
            <i /> {status.state}
          </span>
          <select
            aria-label="Controller COM port"
            value={selectedPort}
            disabled={connected || busy}
            onChange={(event) => setSelectedPort(event.target.value)}
          >
            {status.availablePorts.length === 0 && (
              <option value={selectedPort}>{selectedPort}</option>
            )}
            {status.availablePorts.map((port) => (
              <option key={port} value={port}>
                {port}
              </option>
            ))}
          </select>
          <button type="button" className="controller-refresh" disabled={busy} onClick={refresh}>
            Refresh
          </button>
          {connected ? (
            <button
              type="button"
              className="controller-connect controller-connect--disconnect"
              disabled={busy}
              onClick={() => void handleDisconnect()}
            >
              Disconnect
            </button>
          ) : (
            <button
              type="button"
              className="controller-connect"
              disabled={busy || scenarioRunning}
              title={scenarioRunning ? "Stop automation before manual control" : undefined}
              onClick={() => void handleConnect()}
            >
              {busy ? "Connecting…" : "Connect"}
            </button>
          )}
        </div>
      </div>

      <div className="gamepad">
        <div className="gamepad-side gamepad-side--left">
          <div className="shoulder-row">
            <ControllerButton button="ZL" label="ZL" className="shoulder-button" {...buttonProps} />
            <ControllerButton button="L" label="L" className="shoulder-button" {...buttonProps} />
          </div>
          <fieldset className="dpad">
            <legend className="visually-hidden">Directional pad</legend>
            <ControllerButton button="UP" label="▲" className="dpad-up" {...buttonProps} />
            <ControllerButton button="LEFT" label="◀" className="dpad-left" {...buttonProps} />
            <span className="dpad-center" />
            <ControllerButton button="RIGHT" label="▶" className="dpad-right" {...buttonProps} />
            <ControllerButton button="DOWN" label="▼" className="dpad-down" {...buttonProps} />
          </fieldset>
        </div>

        <AnalogStick
          stick="left"
          label="L STICK"
          disabled={controlsDisabled}
          onChange={handleStick}
        />

        <div className="gamepad-center">
          <div className="utility-row">
            <ControllerButton button="MINUS" label="−" {...buttonProps} />
            <ControllerButton button="CAPTURE" label="□" {...buttonProps} />
            <ControllerButton button="HOME" label="⌂" {...buttonProps} />
            <ControllerButton button="PLUS" label="＋" {...buttonProps} />
          </div>
          <div className="stick-click-row">
            <ControllerButton button="L_STICK" label="L3" {...buttonProps} />
            <ControllerButton button="R_STICK" label="R3" {...buttonProps} />
          </div>
          <button
            type="button"
            className="neutralize-button"
            disabled={controlsDisabled}
            onClick={() => enqueue(neutralizeManualController)}
          >
            Release all
          </button>
        </div>

        <AnalogStick
          stick="right"
          label="R STICK"
          disabled={controlsDisabled}
          onChange={handleStick}
        />

        <div className="gamepad-side gamepad-side--right">
          <div className="shoulder-row">
            <ControllerButton button="R" label="R" className="shoulder-button" {...buttonProps} />
            <ControllerButton button="ZR" label="ZR" className="shoulder-button" {...buttonProps} />
          </div>
          <fieldset className="face-buttons">
            <legend className="visually-hidden">Face buttons</legend>
            <ControllerButton button="X" label="X" className="face-x" {...buttonProps} />
            <ControllerButton button="Y" label="Y" className="face-y" {...buttonProps} />
            <ControllerButton button="A" label="A" className="face-a" {...buttonProps} />
            <ControllerButton button="B" label="B" className="face-b" {...buttonProps} />
          </fieldset>
        </div>
      </div>

      {(inputError || status.error) && (
        <p className="controller-error">{inputError ?? status.error}</p>
      )}
    </section>
  );
}
