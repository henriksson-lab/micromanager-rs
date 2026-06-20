# SquidPlusObjectiveTurret Protocol Specification

2-position objective turret for the Squid+ microscope, driven by a Xeryon
XLS-1250-3N linear stage.

Reference implementation: `squid-control/squid_control/hardware/objective_switcher.py`

## Hardware

- **Stage:** Xeryon XLS-1250-3N linear stage
- **Axes:** X only (single-axis)
- **Encoder resolution:** 1250 nm/count
- **USB VID/PID:** 0x04D8 / 0xEF4A

## Transport Layer

Same as the Xeryon serial protocol (see `protocols/xeryon.md`).

- **Interface:** USB serial
- **Baud rate:** 115200
- **Data bits:** 8, **Stop bits:** 1, **Parity:** None
- **Line terminator:** `\n` (sent), `\r\n` (received)

## Message Format

Text-based, axis-prefixed TAG=VALUE:

```
Host → Controller:   X:TAG=VALUE\n
Controller → Host:   X:TAG=VALUE\r\n
```

Values are integers (encoder counts, speed units, etc.).

## Position Map

| Position | Physical offset | Encoder counts | Default label |
|----------|-----------------|----------------|---------------|
| 0        | −19.0 mm        | −15200         | Pos-1         |
| 1        | +19.0 mm        | +15200         | Pos-2         |

Count conversion: `counts = mm × 1 000 000 / 1250`

## Initialization Sequence

| Step | Command sent       | Expected response   | Purpose                          |
|------|--------------------|---------------------|----------------------------------|
| 1    | `X:RSET=0\n`       | (none, fire-and-forget) | Reset axis                   |
| 2    | `XLS3=1251\n`      | (none, fire-and-forget) | Set encoder resolution       |
| 3    | `X:SSPD=1000\n`    | (none, fire-and-forget) | Set speed (1.0 mm/s × 1000) |
| 4    | `X:INDX=0\n`       | `X:INDX=...\r\n`   | Home via findIndex (blocking)    |
| 5    | `X:EPOS=0\n`       | `X:EPOS=<counts>\r\n` | Read encoder position          |

After step 5 the driver snaps to the nearest discrete position (0 or 1) based
on the reported encoder count.

**Note on homing:** The Python squid-control reference uses `findIndex`
(`X:INDX`) for homing, which differs from the original C++ MicroManager Xeryon
adapter (which uses `X:ZERO`).

## Position Change

To move to position *p* (0 or 1):

```
X:DPOS=<counts>\n
```

Where `<counts>` is the signed encoder-count value from the position map above
(−15200 or +15200).

The controller responds with:

```
X:DPOS=<counts>\r\n
```

## Shutdown

```
X:STOP=1\n          (fire-and-forget)
```

## Configuration Properties

| Property           | Type   | Default      | Pre-init | Description                     |
|--------------------|--------|--------------|----------|---------------------------------|
| Port               | String | Undefined    | yes      | Serial port name                |
| EncoderResolution  | String | XLS3=1251    | yes      | Encoder resolution command      |
| Speed_mm_per_s     | Float  | 1.0          | yes      | Movement speed in mm/s          |

## Relevant Commands Summary

| Tag    | Value          | Direction     | Description                  |
|--------|----------------|---------------|------------------------------|
| `RSET` | 0              | host → ctrl   | Reset axis                   |
| `SSPD` | speed × 1000   | host → ctrl   | Set speed                    |
| `INDX` | 0              | host → ctrl   | Find encoder index (home)    |
| `EPOS` | 0              | host → ctrl   | Query encoder position       |
| `DPOS` | encoder counts  | host → ctrl   | Set desired position (abs)   |
| `STOP` | 1              | host → ctrl   | Stop movement                |

See `protocols/xeryon.md` for full Xeryon protocol details.
