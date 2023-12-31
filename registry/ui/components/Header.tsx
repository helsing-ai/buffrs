import { forwardRef } from 'react'
import Link from 'next/link'
import clsx from 'clsx'
import { motion, useScroll, useTransform } from 'framer-motion'

import { Button } from '@/components/Button'
import { Logo } from '@/components/Logo'
import { MobileSearch, Search } from '@/components/Search'
import { ThemeToggle } from '@/components/ThemeToggle'

function TopLevelNavItem({
  href,
  children,
}: {
  href: string
  children: React.ReactNode
}) {
  return (
    <li>
      <Link
        href={href}
        className="text-sm leading-5 text-zinc-600 transition hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-white"
      >
        {children}
      </Link>
    </li>
  )
}

export const Header = forwardRef<
  React.ElementRef<'div'>,
  { className?: string }
>(function Header({ className }, ref) {
  let { scrollY } = useScroll()
  let bgOpacityLight = useTransform(scrollY, [0, 72], [0.5, 0.9])
  let bgOpacityDark = useTransform(scrollY, [0, 72], [0.2, 0.8])

  return (
    <motion.div
      ref={ref}
      className={clsx(
        className,
        'fixed inset-x-0 top-0 z-50 h-14 px-4 sm:px-6 lg:px-8',
        'transition lg:z-30',
        'backdrop-blur-sm dark:backdrop-blur',
        'bg-white/[var(--bg-opacity-light)] dark:bg-zinc-900/[var(--bg-opacity-dark)]',
      )}
      style={
        {
          '--bg-opacity-light': bgOpacityLight,
          '--bg-opacity-dark': bgOpacityDark,
        } as React.CSSProperties
      }
    >
    <div className="mx-auto h-full w-full max-w-2xl lg:max-w-5xl">
      <div className="flex h-full w-full items-center justify-between">
        <div
          className={clsx(
            'absolute inset-x-0 top-full h-px transition',
          )}
        />
        <div className="flex items-center gap-5">
          <Link href="/" aria-label="Home">
            <Logo className="h-6" />
          </Link>
        </div>

        <Search />

        <div className="flex items-center gap-5">
          <nav className="hidden md:block">
            <ul role="list" className="flex items-center gap-8">
              <TopLevelNavItem href="https://github.com/helsing-ai/buffrs/issues">Support</TopLevelNavItem>
              <TopLevelNavItem href="https://helsing-ai.github.io/buffrs/">Documentation</TopLevelNavItem>
            </ul>
          </nav>
          <div className="hidden md:block md:h-5 md:w-px md:bg-zinc-900/10 md:dark:bg-white/15" />
          <div className="flex gap-4">
            <MobileSearch />
            <ThemeToggle />
          </div>
          <div className="hidden min-[416px]:contents">
            <Button href="#">Sign in</Button>
          </div>
        </div>
      </div>
      </div>
    </motion.div>
  )
})
