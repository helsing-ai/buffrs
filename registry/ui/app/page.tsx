import { Button } from '@/components/Button'
import { Hero } from '@/components/hero/Hero'

import { Statistics } from '@/components/Statistics'
import { Packages } from '@/components/Packages'
import { Categories } from '@/components/Categories'

export default function Index() {
  return (
    <>
      <Hero />
      <Statistics />
      <Packages />
      <Categories />
    </>
  )
}
