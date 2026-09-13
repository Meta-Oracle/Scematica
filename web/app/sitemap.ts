import type { MetadataRoute } from 'next'

import { PRODUCTS } from '@/components/products'

// Every product on scematica.org, in one place.
//
// Ten routes: the dashboard plus nine products. The nav no longer hides them on a phone
// — see `components/ProductNav` — but a sitemap is what makes a page findable by somebody
// who has never seen the nav at all.
//
// `NEXT_PUBLIC_SITE_URL` so a preview deployment does not advertise production URLs — a
// sitemap that points somewhere else is worse than none, because a crawler believes it.
const BASE = (process.env.NEXT_PUBLIC_SITE_URL ?? 'https://scematica.org').replace(/\/+$/, '')

/**
 * Route, and how often it is worth re-reading.
 *
 * The products come from `components/products.ts`, the same table the nav renders, so a
 * page cannot exist and be unfindable — the quieter half of the failure a link that
 * overflows off-screen produces loudly. Only `/` is listed here, because it is not a
 * product.
 */
const ROUTES: [string, MetadataRoute.Sitemap[number]['changeFrequency'], number][] = [
  ['/', 'hourly', 1.0],
  ...PRODUCTS.map(
    p => [p.href, p.changeFrequency, p.priority] as [string, MetadataRoute.Sitemap[number]['changeFrequency'], number],
  ),
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
