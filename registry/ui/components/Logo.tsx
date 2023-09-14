type LogoProps = { className?: string; };

export const Logo = ({ className }: LogoProps) => (
    <p className={"font-bold " + (className ?? '')}>Buffrs</p>
);
