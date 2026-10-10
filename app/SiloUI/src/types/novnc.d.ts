declare module "@novnc/novnc" {
  export interface RFBCredentials {
    username?: string
    password?: string
    target?: string
  }
  export interface RFBOptions {
    shared?: boolean
    credentials?: RFBCredentials
    repeaterID?: string
    wsProtocols?: string[]
  }
  export default class RFB {
    constructor(target: HTMLElement, urlOrChannel: string | WebSocket | RTCDataChannel, options?: RFBOptions)
    scaleViewport: boolean
    clipViewport: boolean
    resizeSession: boolean
    showDotCursor: boolean
    viewOnly: boolean
    focusOnClick: boolean
    qualityLevel: number
    compressionLevel: number
    disconnect(): void
    approveServer(): void
    sendCredentials(credentials: RFBCredentials): void
    focus(options?: FocusOptions): void
    addEventListener(type: string, listener: (event: CustomEvent) => void): void
    removeEventListener(type: string, listener: (event: CustomEvent) => void): void
  }
}
