'use client'

import { useCallback, useEffect, useRef, useState } from 'react'

import {
  estimateWordMs,
  pickVoice,
  speakableText,
  speechSupported,
  splitForSpeech,
  wordAt,
} from '@/lib/scylar/speech'

// The speech engine. Everything decidable without a browser lives in lib/scylar/speech.ts;
// this is the part that has to touch `window.speechSynthesis`.
//
// It reports word boundaries as they fire so the caller can drive the avatar's mouth
// from them. The alternative — letting the avatar own the utterance — would put a
// browser API inside a component whose entire value is being a pure function of state.

export interface SpeechWord {
  /** Predicted duration of the word that just started, in ms. */
  wordMs: number
  /** Bumped per boundary so an identical duration still reads as a new word. */
  seq: number
}

export interface Speech {
  supported: boolean
  speaking: boolean
  /** Latest word boundary, or null when not speaking. */
  word: SpeechWord | null
  speak: (markdown: string) => void
  cancel: () => void
}

export function useSpeech(enabled: boolean): Speech {
  const [supported, setSupported] = useState(false)
  const [speaking, setSpeaking] = useState(false)
  const [word, setWord] = useState<SpeechWord | null>(null)

  const voiceRef = useRef<SpeechSynthesisVoice | null>(null)
  const seqRef = useRef(0)
  // Utterances must stay referenced for the lifetime of the queue: several browsers
  // garbage-collect a still-speaking utterance whose only reference was local, which
  // presents as speech stopping partway through for no visible reason.
  const queueRef = useRef<SpeechSynthesisUtterance[]>([])

  useEffect(() => {
    if (!speechSupported()) return
    setSupported(true)

    // The voice list is frequently empty on first read and populated asynchronously, so
    // this has to run on the event as well as immediately.
    const load = () => {
      voiceRef.current = pickVoice(window.speechSynthesis.getVoices())
    }
    load()
    window.speechSynthesis.addEventListener('voiceschanged', load)
    return () => window.speechSynthesis.removeEventListener('voiceschanged', load)
  }, [])

  const cancel = useCallback(() => {
    if (!speechSupported()) return
    window.speechSynthesis.cancel()
    queueRef.current = []
    setSpeaking(false)
    setWord(null)
  }, [])

  // Leaving the page mid-sentence otherwise keeps talking: speechSynthesis is global to
  // the tab and outlives the component that started it.
  useEffect(() => cancel, [cancel])

  /**
   * Watchdog.
   *
   * `onend` is not dependable — Chrome drops it when the queue is cancelled from
   * elsewhere, when the tab is backgrounded mid-utterance, and occasionally for no
   * reason at all. A missed `onend` would leave `speaking` stuck true, and the caller
   * treats speaking as busy, so the whole terminal would lock with the mouth open. The
   * engine's own `speaking`/`pending` flags are the ground truth; this polls them and
   * only while we believe she is talking, so an idle page runs no timer.
   */
  useEffect(() => {
    if (!speaking || !speechSupported()) return
    const id = window.setInterval(() => {
      const s = window.speechSynthesis
      if (!s.speaking && !s.pending) {
        setSpeaking(false)
        setWord(null)
      }
    }, 400)
    return () => window.clearInterval(id)
  }, [speaking])

  // Turning voice off should stop her immediately, not at the end of the paragraph.
  useEffect(() => {
    if (!enabled) cancel()
  }, [enabled, cancel])

  const speak = useCallback(
    (markdown: string) => {
      if (!enabled || !speechSupported()) return

      const text = speakableText(markdown)
      const chunks = splitForSpeech(text)
      if (chunks.length === 0) return

      window.speechSynthesis.cancel()
      queueRef.current = []
      setSpeaking(true)

      chunks.forEach((chunk, i) => {
        const u = new SpeechSynthesisUtterance(chunk)
        if (voiceRef.current) u.voice = voiceRef.current
        u.rate = 1.02
        u.pitch = 1.05

        u.onboundary = (e) => {
          // Some engines fire boundaries for punctuation and sentences too. Only word
          // boundaries should move the mouth; the rest would open it on a comma.
          if (e.name && e.name !== 'word') return
          const w = wordAt(chunk, e.charIndex)
          if (!w) return
          seqRef.current += 1
          setWord({ wordMs: estimateWordMs(w, u.rate), seq: seqRef.current })
        }

        // Only the last chunk ends the turn — the queue is one utterance per sentence.
        if (i === chunks.length - 1) {
          u.onend = () => {
            setSpeaking(false)
            setWord(null)
          }
        }

        // `onerror` fires on cancel as well as on failure. Either way the right response
        // is to stop claiming she is speaking; a stuck `speaking` flag would leave the
        // mouth open and the input disabled.
        u.onerror = () => {
          setSpeaking(false)
          setWord(null)
        }

        queueRef.current.push(u)
        window.speechSynthesis.speak(u)
      })
    },
    [enabled],
  )

  return { supported, speaking, word, speak, cancel }
}
