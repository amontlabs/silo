const platform = () => (typeof navigator === "undefined" ? "" : navigator.platform)

export const isMac = () => /^Mac/i.test(platform())

export const isLinux = () => /^Linux/i.test(platform())
