import type { MetadataRoute } from 'next'

// Every product on scematica.org, in one place.
//
// There are eight now and the header nav is `hidden md:flex`, so on a phone none of them are
// linked at all. A sitemap is not a substitute for navigation, but it is what makes the pages
// findable rather than reachable only by somebody who already knows the URL.
//
// `NEXT_PUBLIC_SITE_URL` so a preview deployment does not advertise production URLs — a
// sitemap that points somewhere else is worse than none, because a crawler believes it.
const BASE = (process.env.NEXT_PUBLIC_SITE_URL ?? 'https://scematica.org').replace(/\/+$/, '')

/** Route, and how often it is worth re-reading. */
const ROUTES: [string, MetadataRoute.Sitemap[number]['changeFrequency'], number][] = [
  ['/', 'hourly', 1.0],
  ['/scema-world', 'weekly', 0.9],
  ['/omni', 'weekly', 0.9],
  ['/alchem-link', 'daily', 0.8],
  ['/escrow', 'daily', 0.8],
  ['/mesh', 'hourly', 0.7],
  ['/scylar-terminal', 'weekly', 0.7],
  ['/botchain', 'weekly', 0.6],
]

export default function sitemap(): MetadataRoute.Sitemap {
  // No `lastModified`. It would have to come from a clock at build time, which makes every
  // rebuild claim every page changed — the same reason `scema-nft` has no "minted at" field.
  return ROUTES.map(([path, changeFrequency, priority]) => ({
    url: `${BASE}${path}`,
    changeFrequency,
    priority,
  }))
}
