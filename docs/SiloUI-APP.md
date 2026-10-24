# Using the Silo app

The Silo app is a desktop app for Apple Silicon Macs built on the Silo engine. To install it, see the [repository README](../README.md#install).

## Start working

1. **Create a computer.** Follow setup to choose its name, CPU, memory, and disk size. GitHub is optional. The Linux desktop and computer use are built in. Use **Add → New computer** to create more later.
2. **Open your project.** Start the computer and open its terminal. Create or clone your project in `/workspace`; use HTTPS URLs for Silo's GitHub integration. In **Files**, open a folder in your preferred editor.
3. **Open a development server.** Run it inside the computer, listening on `0.0.0.0`. In **Network**, connect a discovered port or choose **Add port**, then open the displayed address on your device.
4. **Open the desktop.** Choose **Open Linux desktop** from the computer's actions. Closing the viewer leaves its graphical apps running. A computer created before the built-in desktop keeps its current setup: add a desktop with **Add Linux desktop**, then choose **Set up LCU** in the viewer, which needs the official ChatGPT Linux app inside the computer. Create a new computer for automatic computer use.

Stopping a computer ends its running programs and preserves its files. Names and disk sizes are fixed after creation; changing CPU or memory stops the computer and applies on its next start. Quitting Silo stops local computers; computers on other devices keep running.

## Connect another device

Install the same version of Silo on both devices; a device with an older version must update before they connect. You can connect during setup without creating a local computer.

1. On the device that will run the computers, enable SSH access (**Remote Login** on macOS). In **Settings → Connections**, enable **Allow connections from other devices** and copy the address.
2. On your device, choose **Add → Connect device…**, paste the address, and follow the SSH setup prompts.
3. Use its computers alongside your local ones. Choose **Run on** when creating a computer to select its device.

Keep Silo running on the device that owns the computers. Manage its GitHub account, secrets, exports, and imports there. See [Connections](SiloUI-CONNECTIONS.md) for details.
