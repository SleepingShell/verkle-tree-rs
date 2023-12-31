use ark_poly::GeneralEvaluationDomain;
use once_cell::sync::Lazy;
use pprof::criterion::{Output, PProfProfiler};
use rand::{thread_rng, Rng};
use sha2::Sha256;
use vector_commit::{
    ipa::*,
    lagrange_basis::LagrangeBasis,
    multiproof::{MultiproofProverQuery, MultiproofVerifierQuery, VectorCommitmentMultiproof},
    VCData, VectorCommitment,
};

use ark_bn254::Bn254;
use ark_ec::pairing::Pairing;
use ark_ff::{field_hashers::DefaultFieldHasher, PrimeField, UniformRand};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

const SIZE: usize = 256;
const MAX_MULTIPROOF: usize = 2usize.pow(15);

type F = <Bn254 as Pairing>::ScalarField;
type G1 = <Bn254 as Pairing>::G1;
type Hasher = DefaultFieldHasher<Sha256>;
type D = GeneralEvaluationDomain<F>;

type IPAT = IPA<256, G1, Hasher, GeneralEvaluationDomain<F>>;

struct TestData {
    crs: IPAUniversalParams<SIZE, G1, Hasher>,
    data: Vec<(LagrangeBasis<F, D>, IPACommitment<G1>, usize, F)>,
}

static DATA: Lazy<TestData> = Lazy::new(|| {
    let mut rng = thread_rng();
    let mut point_gen = IPAPointGenerator::default();
    point_gen.set_max(512);
    let crs = IPAT::setup(SIZE, &point_gen).unwrap();
    let data = (0..MAX_MULTIPROOF as usize)
        .map(|i| {
            let data = gen_data(SIZE);
            let commit = IPAT::commit(&crs, &data).unwrap();
            let challenge = rng.gen_range(0..SIZE);
            let eval = data[challenge];
            if i % (MAX_MULTIPROOF / 10) == 0 {
                println!("{}% data generated", (i * 100 / MAX_MULTIPROOF));
            }
            (data, commit, challenge, eval)
        })
        .collect::<Vec<_>>();

    TestData { crs, data }
});

fn gen_data(num: usize) -> LagrangeBasis<F, GeneralEvaluationDomain<F>> {
    let mut data: Vec<F> = vec![];
    let mut rng = thread_rng();
    let r_f = F::rand(&mut rng);
    for i in 0..num {
        data.push(r_f + F::from(i as u64));
    }
    LagrangeBasis::<F, GeneralEvaluationDomain<F>>::from_vec(data)
}

fn setup(
    n: usize,
    max_degree: usize,
) -> (
    LagrangeBasis<F, GeneralEvaluationDomain<F>>,
    IPAUniversalParams<SIZE, G1, Hasher>,
) {
    let data = gen_data(n);
    let mut point_gen = IPAPointGenerator::default();
    point_gen.set_max(512);
    let crs = IPAT::setup(max_degree, &point_gen).unwrap();

    (data, crs)
}

fn bench_commitment(c: &mut Criterion) {
    let (data, crs) = setup(SIZE, SIZE);

    c.bench_function("ipa_commitment", |b| b.iter(|| IPAT::commit(&crs, &data)));
}

fn bench_prove_single(c: &mut Criterion) {
    let (data, crs) = setup(SIZE, SIZE);
    let commit = IPAT::commit(&crs, &data).unwrap();

    c.bench_function("ipa_prove_single_in_domain", |b| {
        let index = thread_rng().gen_range(0..SIZE);
        b.iter(|| IPAT::prove(&crs, &commit, index, &data))
    });

    c.bench_function("ipa_prove_single_out_domain", |b| {
        let index = thread_rng().gen_range(SIZE..SIZE * 16);
        b.iter(|| IPAT::prove(&crs, &commit, index, &data))
    });
}

fn bench_verify_single(c: &mut Criterion) {
    let (data, crs) = setup(SIZE, SIZE);
    let commit = IPAT::commit(&crs, &data).unwrap();

    c.bench_function("ipa_verify_single_in_domain", |b| {
        let index = thread_rng().gen_range(0..SIZE);
        let proof = IPAT::prove(&crs, &commit, index, &data).unwrap();
        b.iter(|| IPAT::verify(&crs, &commit, index, &proof))
    });
}

fn bench_prove_multiproof(c: &mut Criterion) {
    let base = MAX_MULTIPROOF / 8;
    let mut group = c.benchmark_group("ipa_multiproof_prove");
    group.sample_size(10);

    let queries = DATA
        .data
        .iter()
        .map(|(d, c, z, y)| MultiproofProverQuery::new(d, c, *z, *y))
        .collect::<Vec<_>>();

    for size in [base, base * 4, base * 8].iter() {
        group.throughput(criterion::Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &queries[0..*size],
            |b, q| {
                b.iter(|| IPAT::prove_multiproof(&DATA.crs, q));
            },
        );
    }
}

fn bench_verify_multiproof(c: &mut Criterion) {
    let base = MAX_MULTIPROOF / 8;

    let mut group = c.benchmark_group("ipa_multiproof_verify");
    group.sample_size(10);

    let queries: Vec<_> = DATA
        .data
        .iter()
        .map(|(d, c, z, y)| MultiproofProverQuery::new(d, c, *z, *y))
        .collect();
    let verifier_queries: Vec<_> = queries.iter().map(|q| q.to_verifier_query()).collect();

    for size in [base, base * 4, base * 8].iter() {
        let proof = IPAT::prove_multiproof(&DATA.crs, &queries[0..*size]).unwrap();

        group.throughput(criterion::Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(&verifier_queries[0..*size], &proof),
            |b, (q, p)| {
                b.iter(|| IPAT::verify_multiproof(&DATA.crs, *q, *p));
            },
        );
    }
}

criterion_group! {
    name = ipa_proofs;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = bench_commitment,
    bench_prove_single,
    bench_verify_single,
    bench_prove_multiproof,
    bench_verify_multiproof
}
criterion_main!(ipa_proofs);
