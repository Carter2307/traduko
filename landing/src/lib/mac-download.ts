export const MAC_DOWNLOADS = {
  appleSilicon: 'https://github.com/Carter2307/traduko/releases/download/v0.2.0/Traduko-0.2.0-macos-apple-silicon.zip',
  intel: 'https://github.com/Carter2307/traduko/releases/download/v0.2.0/Traduko-0.2.0-macos-intel.zip',
} as const

export interface MacClientHints {
  platform?: string
  getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string; bitness?: string }>
}

export async function resolveMacDownloadUrl(clientHints?: MacClientHints): Promise<string> {
  if (clientHints?.platform !== 'macOS' || !clientHints.getHighEntropyValues) {
    return MAC_DOWNLOADS.appleSilicon
  }

  let timeout: ReturnType<typeof setTimeout> | undefined
  try {
    const hints = await Promise.race([
      clientHints.getHighEntropyValues(['architecture', 'bitness']),
      new Promise<undefined>((resolve) => { timeout = setTimeout(() => resolve(undefined), 1000) }),
    ])

    return hints?.architecture === 'x86' && hints.bitness === '64'
      ? MAC_DOWNLOADS.intel
      : MAC_DOWNLOADS.appleSilicon
  } catch {
    return MAC_DOWNLOADS.appleSilicon
  } finally {
    if (timeout !== undefined) clearTimeout(timeout)
  }
}
