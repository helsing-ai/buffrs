import { Button } from '@/components/Button';
import { HeroPattern } from '@/components/hero/HeroPattern';

type HeroProps = { className?: string; };

export const Hero = ({ className }: HeroProps) => (
    <>
        <HeroPattern />

        <div className="mt-16">
        <h1 className="mt-2 text-4xl font-bold text-zinc-900 dark:text-white">
            Protobuf Package Registry
        </h1>

        <div className="mb-16 mt-6 flex gap-3">
          <Button href="https://helsing-ai.github.io/buffrs/getting-started/installation.html" arrow="down">Install Buffrs</Button>
          <Button href="https://helsing-ai.github.io/buffrs/guide/index.html" variant="outline">Read The Guide</Button>
        </div>
        </div>
    </>
);
