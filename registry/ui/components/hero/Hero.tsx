import { Button } from '@/components/Button';
import { HeroPattern } from '@/components/hero/HeroPattern';

type HeroProps = { className?: string; title: string; children?: React.Node; };

export const Hero = ({ title, children, className }: HeroProps) => (
    <>
        <HeroPattern />

        <div className="my-16">
            <h1 className="mt-2 text-4xl font-bold text-zinc-900 dark:text-white">
                {title}
            </h1>

            {children && (
                <div className="mb-16 mt-6 flex gap-3">
                    {children}
                </div>
            )}
        </div>
    </>
);
