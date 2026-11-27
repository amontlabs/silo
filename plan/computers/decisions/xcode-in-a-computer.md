# How does Xcode get into a macOS computer

## The choice

An agent's part, 24 November 2026. You build an iPhone app. Main is a fresh macOS computer, and
your agents need Xcode 27.2 and the iOS 27.3 simulator in it before they can build anything.
Apple's download of Xcode needs your Apple Account with two-factor sign-in, and the Mac App Store
cannot be signed into in a virtual machine. Simulator runtimes come from Apple with no account.

The options:

- Copy it in from your Mac (recommended). You already have Xcode, or download it once on your Mac
  with your own account. Silo copies it into the computer, checks Apple's signature on the copy,
  accepts the license and runs the first launch over SSH, then fetches the runtime. It costs the
  copy, several GB once per Xcode version, and it assumes you have Xcode on your Mac. Silo keeps
  the result, so later macOS computers start from a clone with Xcode already in.
- Your Apple Account in the computer. A tool such as `xcodes` downloads Xcode in the computer
  with your account. It costs your Apple Account's session inside a computer where an agent has
  `sudo`, and a two-factor code that someone types each time.
- You install it by hand, on the computer's screen. It costs your time at each new computer, and
  nothing Silo can repeat.

Recommended: copy it in from your Mac. Nothing of your account goes in, and Apple's signature
shows the copy is Apple's Xcode.

## Notes

An agent's research, 24 November 2026: `research/2026-11-24-mac-and-ios-developers/README.md`, section "Xcode in a
computer". The sizes of the runtimes were read from Apple's own index: the iOS 27.3 runtime for
Apple silicon is 9.0 GB. Mac test 23 of the kit measures the copy, the signature check, the first
launch over SSH and the size of Xcode 27 in a computer.

A copy of Xcode is an import you ask for, as `files-imported-from-your-mac` says. A simulator
runtime is signed by Apple, so a store of runtimes that only Silo fills fits what `shared-cache`
allows.
