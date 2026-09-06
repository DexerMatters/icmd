use crossbeam_channel::{Receiver, Sender, bounded};
use std::thread;

/// A typed, threaded element of a pipeline.
pub trait Component: Send + 'static {
    type Input: Send + 'static;
    type Output: Send + 'static;

    fn run(self, input: Receiver<Self::Input>, output: Sender<Self::Output>);

    fn spawn(self, capacity: usize) -> Endpoint<Self::Input, Self::Output>
    where
        Self: Sized,
    {
        let capacity = capacity.max(1);
        let (input, input_rx) = bounded(capacity);
        let (output, output_rx) = bounded(capacity);
        thread::spawn(move || self.run(input_rx, output));
        Endpoint {
            input,
            output: output_rx,
        }
    }
}

/// The input and output ends of one component or connected pipeline.
pub struct Endpoint<I: Send + 'static, O: Send + 'static> {
    input: Sender<I>,
    output: Receiver<O>,
}

impl<I: Send + 'static, O: Send + 'static> Endpoint<I, O> {
    pub fn input(&self) -> Sender<I> {
        self.input.clone()
    }

    pub fn output(self) -> Receiver<O> {
        self.output
    }

    pub fn into_parts(self) -> (Sender<I>, Receiver<O>) {
        (self.input, self.output)
    }
}

#[doc(hidden)]
pub struct End;

#[doc(hidden)]
pub struct Chain<Head, Tail>(Head, Tail);

#[doc(hidden)]
pub trait Append<Next> {
    type Output;
    fn append(self, next: Next) -> Self::Output;
}

impl<Next> Append<Next> for End {
    type Output = Chain<Next, End>;

    fn append(self, next: Next) -> Self::Output {
        Chain(next, End)
    }
}

impl<Head, Tail, Next> Append<Next> for Chain<Head, Tail>
where
    Tail: Append<Next>,
{
    type Output = Chain<Head, <Tail as Append<Next>>::Output>;

    fn append(self, next: Next) -> Self::Output {
        let Chain(head, tail) = self;
        Chain(head, tail.append(next))
    }
}

#[doc(hidden)]
pub trait StartChain {
    type Input: Send + 'static;
    type Output: Send + 'static;

    fn start(self, capacity: usize) -> Endpoint<Self::Input, Self::Output>;
}

impl<C> StartChain for Chain<C, End>
where
    C: Component,
{
    type Input = C::Input;
    type Output = C::Output;

    fn start(self, capacity: usize) -> Endpoint<Self::Input, Self::Output> {
        let Chain(component, End) = self;
        component.spawn(capacity)
    }
}

impl<Head, Tail> StartChain for Chain<Head, Tail>
where
    Head: Component,
    Tail: StartChain<Input = Head::Output>,
{
    type Input = Head::Input;
    type Output = Tail::Output;

    fn start(self, capacity: usize) -> Endpoint<Self::Input, Self::Output> {
        let Chain(head, tail) = self;
        let upstream = head.spawn(capacity);
        let downstream = tail.start(capacity);
        connect(upstream, downstream)
    }
}

fn connect<I, M, O>(upstream: Endpoint<I, M>, downstream: Endpoint<M, O>) -> Endpoint<I, O>
where
    I: Send + 'static,
    M: Send + 'static,
    O: Send + 'static,
{
    let Endpoint { input, output } = upstream;
    let Endpoint {
        input: next_input,
        output: next_output,
    } = downstream;
    thread::spawn(move || {
        while let Ok(value) = output.recv() {
            if next_input.send(value).is_err() {
                break;
            }
        }
    });
    Endpoint {
        input,
        output: next_output,
    }
}

/// Describes and starts a typed component pipeline.
pub struct Runtime<C> {
    chain: C,
    capacity: usize,
}

impl<C> Runtime<Chain<C, End>>
where
    C: Component,
{
    pub fn new(component: C) -> Self {
        Self {
            chain: Chain(component, End),
            capacity: 64,
        }
    }

    pub fn with_capacity(component: C, capacity: usize) -> Self {
        Self {
            chain: Chain(component, End),
            capacity: capacity.max(1),
        }
    }
}

impl<C> Runtime<C> {
    pub fn then<Next>(self, next: Next) -> Runtime<<C as Append<Next>>::Output>
    where
        C: Append<Next>,
        Next: Component,
    {
        Runtime {
            chain: self.chain.append(next),
            capacity: self.capacity,
        }
    }
}

impl<C> Runtime<C>
where
    C: StartChain,
{
    pub fn start(self) -> (Sender<C::Input>, Receiver<C::Output>) {
        self.chain.start(self.capacity).into_parts()
    }
}
