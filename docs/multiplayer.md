# Multiplayer

Online multiplayer has been reported working with Flightdeck on Linux. The steps
below help check the same behavior on another setup and diagnose connection issues.

## Test shared flight first

1. In **Settings → General → Online**, enable **Multiplayer**.
2. Both players select the same server.
3. In **Free Flight → Flight Conditions → Multiplayer**, select **All Players**.
4. Load the same airport at nearby, separate parking positions and check whether
   the other aircraft is visible.

These settings follow the [MSFS 2024 guide](https://www.flightsimulator.com/accessibility-msfs-2024/)
and [official community flight instructions](https://forums.flightsimulator.com/t/official-community-fly-in-friday-sports/756745).

## Test groups separately

Use **Social → online friend → Group invite**. The recipient accepts through
**Notifications**. Check group membership and aircraft visibility independently.
The compatibility layer still lacks parts of the platform invitation and
protocol-activation path; joining a public flight does not establish group or
external invitation support.

When reporting a problem, include the selected multiplayer mode, whether both
players chose the same server, the step that failed and the displayed error.
Do not share account tokens or raw authenticated logs. Flightdeck's diagnostic
export is designed to omit those details.
