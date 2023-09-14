import { Button } from '@/components/Button'
import { Hero } from '@/components/hero/Hero'

import { Statistics } from '@/components/Statistics'
import { Packages } from '@/components/Packages'
import { Categories } from '@/components/Categories'

export default function Index() {
  return (
    <>
      <Hero title="Protobuf Package Registry">
                  <Button href="https://helsing-ai.github.io/buffrs/getting-started/installation.html" arrow="down">Install Buffrs</Button>
                  <Button href="https://helsing-ai.github.io/buffrs/guide/index.html" variant="outline">Read The Guide</Button>
      </Hero>
      <Statistics />
      <Packages />
      <Categories />
    </>
  )
}
